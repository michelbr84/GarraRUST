use std::sync::Arc;

use garraia_agents::tools::Tool;
use garraia_agents::{
    AgentRuntime, AnthropicProvider, BashTool, CodeReviewTool, CohereEmbeddingProvider,
    DeviceExecuteTool, DeviceListTool, DeviceReadTool, DeviceToolsConfig, EmbeddingProvider,
    FileReadTool, FileWriteTool, ListDirTool, LlamaCppProvider, McpManager, NoisePolicy,
    OllamaEmbeddingProvider, OllamaProvider, OpenAiEmbeddingProvider, OpenAiProvider,
    RepoSearchTool, ResilientEmbeddingProvider, RunTestsTool, WebFetchTool, WebSearchTool,
};
use garraia_config::{AppConfig, provider_key_env};
use garraia_db::MemoryStore;
use garraia_hardware::automations::{EngineConfig, TetoRisco};
use garraia_hardware::{
    AutomationEngine, AutomationStore, DeviceRegistry, DeviceStateStore, HaAdapterConfig,
    HaAdapterManager, HardwareEventBus, MqttAdapterConfig, MqttAdapterManager, carregar_automacoes,
};
use tracing::{info, warn};

mod channels;
mod config;
mod discord;
mod google_chat;
#[cfg(target_os = "macos")]
mod imessage;
mod irc;
mod line;
mod matrix;
mod openclaw;
mod signal;
mod slack;
mod teams;
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

/// #1050: o canal Google Chat. Canal push, como o WhatsApp — o `Vec<Arc<_>>`
/// vira estado da rota `/webhooks/google-chat`, nao entrada do
/// `ChannelRegistry`.
pub use google_chat::build_google_chat_channels;

/// #1050: o canal LINE, que tinha `impl Channel`, verificacao de assinatura
/// (#1051) e nenhuma rota. Canal push, como o WhatsApp: o `Vec<Arc<_>>` vira
/// estado da rota `/webhooks/line`, nao entrada do `ChannelRegistry`.
pub use line::build_line_channels;

/// #1050: o canal Microsoft Teams. Canal push, e o unico cujo destino de
/// saida vem do corpo da requisicao — ver `bootstrap::teams` e
/// `garraia_channels::teams::auth`.
pub use teams::build_teams_channels;

// Slice 10.f (GAR-480): iMessage wiring extracted to `bootstrap::imessage` (macOS-only).
#[cfg(target_os = "macos")]
pub use imessage::build_imessage_channels;

/// #1050: o canal IRC, que tinha `impl Channel` e nenhum call-site.
pub use irc::build_irc_channels;

/// #1050: o canal Matrix, que tinha `impl Channel` e sync loop e nenhum call-site.
pub use matrix::build_matrix_channels;

/// #1050: le a config do bridge OpenClaw, que ate agora ninguem lia.
pub use openclaw::{build_openclaw_config, spawn_openclaw_router};

/// #1050: o canal Signal, que tinha `impl Channel` e guard de URL e nenhum call-site.
pub use signal::build_signal_channels;

// Slice 10.g (GAR-691): Telegram wiring + voice handler extracted to `bootstrap::telegram`.
pub use telegram::build_telegram_channels;

const KEY_ENV_VAR_OPENAI: &str = "OPENAI_API_KEY";

/// Um endpoint OpenAI-compativel proprio (LM Studio, vLLM, um gateway interno)
/// nao pede credencial; `api.openai.com` pede. Sem essa distincao o bootstrap
/// mandava o literal "no-key" para os dois casos.
fn is_self_hosted_openai_endpoint(base_url: Option<&str>) -> bool {
    match base_url {
        // Sem `base_url`, o provider assume https://api.openai.com.
        None => false,
        Some(url) => !url.contains("api.openai.com"),
    }
}

/// Pergunta ao provider de embeddings se ele esta de pe, e avisa se nao
/// estiver (#951).
///
/// `health_check()` existia no trait desde sempre e **nunca era chamado**:
/// o boot logava "configured ollama embedding provider" e seguia, mesmo com
/// o Ollama desligado. O operador so descobria quando o recall ja tinha
/// degradado — e, ate o #948, nem entao, porque o erro era engolido.
///
/// Avisa, nao derruba o boot: memoria semantica e um recurso opcional do
/// gateway, e recusar subir por causa dela deixaria o usuario sem chat
/// nenhum por um problema que a reindexacao (#953) conserta depois.
pub async fn warn_if_embeddings_unhealthy(runtime: &AgentRuntime) {
    let Some(provider) = runtime.embedding_provider() else {
        return;
    };

    match provider.health_check().await {
        Ok(true) => {
            info!(
                "embedding provider '{}' (modelo '{}') respondeu ao health check",
                provider.provider_id(),
                provider.model()
            );
        }
        Ok(false) => {
            warn!(
                "embedding provider '{}' (modelo '{}') respondeu, mas se declarou fora: \
                 memorias novas vao nascer sem vetor e o recall cai para o caminho \
                 textual ate ele voltar. Depois de restaurar o servico, reindexe as \
                 entradas sem embedding (#953).",
                provider.provider_id(),
                provider.model()
            );
        }
        // A causa entra no log: o corpo da resposta de erro ja e sanitizado na
        // origem (`sanitize_error_body`), entao dizer o motivo aqui nao arrisca
        // vazar credencial — e sem ele o operador so sabe que "nao respondeu".
        Err(e) => {
            warn!(
                "embedding provider '{}' (modelo '{}') NAO respondeu ao health check: \
                 memorias novas vao nascer sem vetor e o recall cai para o caminho \
                 textual ate ele voltar. Depois de restaurar o servico, reindexe as \
                 entradas sem embedding (#953). Causa: {e}",
                provider.provider_id(),
                provider.model()
            );
        }
    }
}

/// Build a fully-configured `AgentRuntime` from the application config.
pub fn build_agent_runtime(config: &AppConfig) -> AgentRuntime {
    let mut runtime = AgentRuntime::new();
    let mut unreachable_local_providers: Vec<String> = Vec::new();

    // Plan 0250 (GAR-771): if the vault is locked, tell the operator clearly and
    // early — before the "no API key" warnings that are the *symptom*.
    config::warn_if_vault_locked();

    let llm_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            config.timeouts.llm.default_secs,
        ))
        .build()
        .unwrap_or_default();

    // --- LLM Providers ---
    for (name, llm_config) in &config.llm {
        let provider_type = llm_config.provider.as_str();

        // One table lookup and one precedence walk for every provider, rather
        // than the same three lines duplicated into fifteen match arms. The
        // table itself lives in `garraia_config::provider_keys` so that
        // `garraia config check` and `/health` answer "does this provider have
        // a key?" exactly the way the boot path does.
        let key_env = provider_key_env(provider_type);
        let resolved =
            key_env.and_then(|var| resolve_api_key(llm_config.api_key.as_deref(), var, var));

        // Central fail-fast. The old per-arm message said only "set api_key in
        // config or <VAR> env var", which omitted the vault — the very place
        // `garraia init` used to put the key by default, leaving operators
        // staring at "no API key" while the key sat encrypted on disk.
        if let Some(var) = key_env
            && resolved.is_none()
        {
            warn!(
                "skipping {provider_type} provider {name}: no API key. Fix it in any one of \
                 three ways — set `llm.{name}.api_key` in config.yml, export {var}, or \
                 unlock the credential vault by exporting {vault_env}.",
                vault_env = garraia_config::provider_keys::VAULT_PASSPHRASE_ENV
            );
            continue;
        }

        // Past the guard, keyed providers are guaranteed to have resolved a
        // key; keyless ones (`ollama`, `echo`) never had one to resolve, and
        // ignore this binding.
        let api_key = resolved.unwrap_or_default();

        match provider_type {
            "anthropic" => {
                let provider = AnthropicProvider::new(
                    api_key,
                    llm_config.model.clone(),
                    llm_config.base_url.clone(),
                )
                .with_client(llm_client.clone());
                runtime.register_provider(Arc::new(provider));
                info!("configured anthropic provider: {name}");
            }
            "openai" => {
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
                        &sock_addr
                            .parse()
                            .unwrap_or_else(|_| std::net::SocketAddr::from(([127, 0, 0, 1], 1234))),
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
                    api_key,
                    llm_config.model.clone(),
                    llm_config.base_url.clone(),
                )
                .with_client(llm_client.clone());
                runtime.register_provider(Arc::new(provider));
                info!("configured openai provider: {name}");
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
            // Keyless local daemon, mirroring the ollama arm: llama-server
            // exposes the OpenAI-compatible API on its documented default
            // port 8080 (ADR 0016 — Garra Mobile local-first).
            "llamacpp" => {
                let base_url = llm_config
                    .base_url
                    .clone()
                    .unwrap_or_else(|| "http://localhost:8080".to_string());

                let addr = base_url
                    .trim_start_matches("http://")
                    .trim_start_matches("https://")
                    .trim_end_matches('/');
                let sock_addr = if addr.contains(':') {
                    addr.to_string()
                } else {
                    format!("{}:8080", addr)
                };
                match std::net::TcpStream::connect_timeout(
                    &sock_addr
                        .parse()
                        .unwrap_or_else(|_| std::net::SocketAddr::from(([127, 0, 0, 1], 8080))),
                    std::time::Duration::from_secs(2),
                ) {
                    Ok(_) => {
                        info!(
                            "✅ llama.cpp server reachable at {} — registering provider: {name}",
                            base_url
                        );
                    }
                    Err(e) => {
                        warn!(
                            "⚠️  llama.cpp server not reachable at {} — provider registered but \
                             will fail until llama-server starts. Error: {}. \
                             Run: llama-server -m <model>",
                            base_url, e
                        );
                    }
                }

                let provider = LlamaCppProvider::new(
                    llm_config.model.clone(),
                    llm_config.base_url.clone(),
                    None,
                );
                runtime.register_provider(Arc::new(provider));
                info!("configured llamacpp provider: {name}");
            }
            "sansa" => {
                let base_url = llm_config
                    .base_url
                    .clone()
                    .or_else(|| Some("https://api.sansaml.com".to_string()));
                let model = llm_config
                    .model
                    .clone()
                    .or_else(|| Some("sansa-auto".to_string()));
                let provider = OpenAiProvider::new(api_key, model, base_url)
                    .with_client(llm_client.clone())
                    .with_name("sansa");
                runtime.register_provider(Arc::new(provider));
                info!("configured sansa provider: {name}");
            }
            "deepseek" => {
                let base_url = llm_config
                    .base_url
                    .clone()
                    .or_else(|| Some("https://api.deepseek.com".to_string()));
                let model = llm_config
                    .model
                    .clone()
                    .or_else(|| Some("deepseek-chat".to_string()));
                let provider = OpenAiProvider::new(api_key, model, base_url)
                    .with_client(llm_client.clone())
                    .with_name("deepseek");
                runtime.register_provider(Arc::new(provider));
                info!("configured deepseek provider: {name}");
            }
            "mistral" => {
                let base_url = llm_config
                    .base_url
                    .clone()
                    .or_else(|| Some("https://api.mistral.ai".to_string()));
                let model = llm_config
                    .model
                    .clone()
                    .or_else(|| Some("mistral-large-latest".to_string()));
                let provider = OpenAiProvider::new(api_key, model, base_url)
                    .with_client(llm_client.clone())
                    .with_name("mistral");
                runtime.register_provider(Arc::new(provider));
                info!("configured mistral provider: {name}");
            }
            "gemini" => {
                let base_url = llm_config.base_url.clone().or_else(|| {
                    Some("https://generativelanguage.googleapis.com/v1beta/openai/".to_string())
                });
                let model = llm_config
                    .model
                    .clone()
                    .or_else(|| Some("gemini-2.5-flash".to_string()));
                let provider = OpenAiProvider::new(api_key, model, base_url)
                    .with_client(llm_client.clone())
                    .with_name("gemini");
                runtime.register_provider(Arc::new(provider));
                info!("configured gemini provider: {name}");
            }
            "falcon" => {
                let base_url = llm_config
                    .base_url
                    .clone()
                    .or_else(|| Some("https://api.ai71.ai/v1".to_string()));
                let model = llm_config
                    .model
                    .clone()
                    .or_else(|| Some("tiiuae/falcon-180b-chat".to_string()));
                let provider = OpenAiProvider::new(api_key, model, base_url)
                    .with_client(llm_client.clone())
                    .with_name("falcon");
                runtime.register_provider(Arc::new(provider));
                info!("configured falcon provider: {name}");
            }
            "jais" => {
                let base_url = llm_config
                    .base_url
                    .clone()
                    .or_else(|| Some("https://api.core42.ai/v1".to_string()));
                let model = llm_config
                    .model
                    .clone()
                    .or_else(|| Some("jais-adapted-70b-chat".to_string()));
                let provider = OpenAiProvider::new(api_key, model, base_url)
                    .with_client(llm_client.clone())
                    .with_name("jais");
                runtime.register_provider(Arc::new(provider));
                info!("configured jais provider: {name}");
            }
            "qwen" => {
                let base_url = llm_config.base_url.clone().or_else(|| {
                    Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1".to_string())
                });
                let model = llm_config
                    .model
                    .clone()
                    .or_else(|| Some("qwen-plus".to_string()));
                let provider = OpenAiProvider::new(api_key, model, base_url)
                    .with_client(llm_client.clone())
                    .with_name("qwen");
                runtime.register_provider(Arc::new(provider));
                info!("configured qwen provider: {name}");
            }
            "yi" => {
                let base_url = llm_config
                    .base_url
                    .clone()
                    .or_else(|| Some("https://api.lingyiwanwu.com/v1".to_string()));
                let model = llm_config
                    .model
                    .clone()
                    .or_else(|| Some("yi-large".to_string()));
                let provider = OpenAiProvider::new(api_key, model, base_url)
                    .with_client(llm_client.clone())
                    .with_name("yi");
                runtime.register_provider(Arc::new(provider));
                info!("configured yi provider: {name}");
            }
            "cohere" => {
                let base_url = llm_config
                    .base_url
                    .clone()
                    .or_else(|| Some("https://api.cohere.com/compatibility/v1".to_string()));
                let model = llm_config
                    .model
                    .clone()
                    .or_else(|| Some("command-r-plus".to_string()));
                let provider = OpenAiProvider::new(api_key, model, base_url)
                    .with_client(llm_client.clone())
                    .with_name("cohere");
                runtime.register_provider(Arc::new(provider));
                info!("configured cohere provider: {name}");
            }
            "minimax" => {
                let base_url = llm_config
                    .base_url
                    .clone()
                    .or_else(|| Some("https://api.minimaxi.chat/v1".to_string()));
                let model = llm_config
                    .model
                    .clone()
                    .or_else(|| Some("MiniMax-Text-01".to_string()));
                let provider = OpenAiProvider::new(api_key, model, base_url)
                    .with_client(llm_client.clone())
                    .with_name("minimax");
                runtime.register_provider(Arc::new(provider));
                info!("configured minimax provider: {name}");
            }
            "moonshot" => {
                let base_url = llm_config
                    .base_url
                    .clone()
                    .or_else(|| Some("https://api.moonshot.cn/v1".to_string()));
                let model = llm_config
                    .model
                    .clone()
                    .or_else(|| Some("kimi-k2-0711-preview".to_string()));
                let provider = OpenAiProvider::new(api_key, model, base_url)
                    .with_client(llm_client.clone())
                    .with_name("moonshot");
                runtime.register_provider(Arc::new(provider));
                info!("configured moonshot provider: {name}");
            }
            "openrouter" => {
                let base_url = llm_config
                    .base_url
                    .clone()
                    .or_else(|| Some("https://openrouter.ai/api/v1".to_string()));
                let model = llm_config
                    .model
                    .clone()
                    .or_else(|| Some("openai/gpt-4o".to_string()));
                let provider = OpenAiProvider::new(api_key, model, base_url)
                    .with_client(llm_client.clone())
                    .with_name("openrouter");
                runtime.register_provider(Arc::new(provider));
                info!("configured openrouter provider: {name}");
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
    // #1105: a allowlist do operador vale nos dois caminhos — com ou sem canal
    // de confirmacao. E ela que destrava o caso reportado (um CLI de outro
    // agente instalado pelo proprio dono) sem abrir o tier risky inteiro.
    let bash_tool = if config.agent.tool_confirmation_enabled {
        BashTool::new_with_confirmation(None)
    } else {
        BashTool::new(None)
    }
    .with_allowlist(config.agent.bash_allowlist.clone());
    runtime.register_tool(Box::new(bash_tool));
    runtime.register_tool(Box::new(FileReadTool::new(None)));
    runtime.register_tool(Box::new(FileWriteTool::new(None)));
    runtime.register_tool(Box::new(WebFetchTool::new(None)));

    // #1033 / #1035: estas tres existiam, com schema e testes verdes, e nunca
    // entraram no runtime — o unico `new()` delas no repo era dentro dos
    // proprios modulos de teste. As whitelists dos modos (`search`, `debug`,
    // `review`) ja anunciavam `list_dir` e `repo_search`; o modelo via a
    // promessa na policy e nao recebia a ferramenta.
    runtime.register_tool(Box::new(ListDirTool::new(None)));
    runtime.register_tool(Box::new(RepoSearchTool::new(None, None)));
    // `run_tests` executa o que o projeto mandar (`npm test` roda o script do
    // package.json), entao respeita a mesma chave de confirmacao do bash.
    let run_tests = if config.agent.tool_confirmation_enabled {
        RunTestsTool::new_with_confirmation(None)
    } else {
        RunTestsTool::new(None)
    };
    runtime.register_tool(Box::new(run_tests));

    // ADR 0020 / epic #1124: as tools de hardware (device_list/read/execute).
    // O registry nasce vazio — nenhum adaptador físico existe ainda (#1126/
    // #1127/#1130) e o boot de adapters via config é a #1128. Com registry
    // vazio, `device_list` responde "Nenhum dispositivo registrado" e nada
    // de hardware roda: o fail-closed é o estado default do deploy.
    // `device_execute` honra `tool_confirmation_enabled` (mesma chave do
    // bash): sem canal, R3/R4/R5 são fail-closed BLOCKED dentro da tool.
    let device_registry = Arc::new(DeviceRegistry::new());
    // #1126/#1127: com `hardware.mqtt` ou `hardware.home_assistant`
    // configurado, os adapters sobem aqui — descoberta (manifestos retained
    // / `/api/states`), presenca no store compartilhado e eventos de estado.
    // Sem secao, o registry segue vazio: fail-closed, o estado default do
    // deploy.
    let device_state = spawn_hardware_adapters(config, device_registry.clone());
    let device_config = Arc::new(match device_state {
        Some(state) => DeviceToolsConfig::new(device_registry).com_estado(state),
        None => DeviceToolsConfig::new(device_registry),
    });
    runtime.register_tool(Box::new(DeviceListTool::new(device_config.clone())));
    runtime.register_tool(Box::new(DeviceReadTool::new(device_config.clone())));
    let device_execute = if config.agent.tool_confirmation_enabled {
        DeviceExecuteTool::new(device_config)
    } else {
        DeviceExecuteTool::new_without_confirmation(device_config)
    };
    runtime.register_tool(Box::new(device_execute));

    // `code_review` roda um segundo LLM por dentro, entao precisa de um
    // provider resolvido aqui, e nao de `None`. Usa o default do boot: se o
    // operador trocar o default depois, a revisao continua no provider de
    // quando o gateway subiu — aceitavel para uma ferramenta auxiliar, e
    // dito aqui para ninguem procurar o porque.
    match runtime.default_provider_id() {
        Some(pid) => match runtime
            .get_provider(&pid)
            .and_then(|p| p.configured_model().map(str::to_string).map(|m| (p, m)))
        {
            Some((provider, model)) => {
                runtime.register_tool(Box::new(CodeReviewTool::new(provider, model, None)));
            }
            None => info!(
                "code_review not registered: default provider '{pid}' has no configured model"
            ),
        },
        None => info!("code_review not registered: no default provider at boot"),
    }

    // Web search (#1034): Brave (chave) ou SearXNG (URL, sem chave). Sem a
    // secao `agent.web_search`, o comportamento e o de sempre — so com chave
    // do Brave. `GARRAIA_SEARXNG_URL` e o equivalente da env para a URL.
    let brave_config_key = config.llm.get("brave").and_then(|c| c.api_key.clone());
    let brave_key = resolve_api_key(
        brave_config_key.as_deref(),
        "BRAVE_API_KEY",
        "BRAVE_API_KEY",
    );
    let searxng_url = config
        .agent
        .web_search
        .searxng_url
        .clone()
        .or_else(|| std::env::var("GARRAIA_SEARXNG_URL").ok())
        .filter(|u| !u.trim().is_empty());
    match select_web_search_backend(config.agent.web_search.backend, brave_key, searxng_url) {
        Some(backend) => {
            info!(backend = backend.name(), "web_search registrada");
            runtime.register_tool(Box::new(WebSearchTool::with_backend(backend)));
        }
        None if config.agent.web_search.backend.is_some() => warn!(
            "agent.web_search.backend configurado sem a chave/URL que ele precisa; \
             web_search fica de fora (veja `garra config check`)"
        ),
        None => {}
    }

    // --- Memory ---
    if config.memory.enabled {
        let data_dir = config.resolved_data_dir();

        if let Err(e) = std::fs::create_dir_all(&data_dir) {
            warn!("failed to create data directory: {e}");
        }

        // Mesma resolucao que a CLI usa (`garra memory`) — ver
        // `AppConfig::memory_db_path`.
        let memory_db_path = config.memory_db_path();
        match MemoryStore::open(&memory_db_path) {
            Ok(store) => {
                let store = Arc::new(store);
                runtime.set_memory_provider(store);
                info!("memory store opened at {}", memory_db_path.display());

                // Attach embedding provider if configured. A construcao mora
                // em `build_embedding_provider` para a CLI reusar o mesmo
                // provider sem subir um runtime (#953).
                if let Some(provider) = build_embedding_provider(config) {
                    runtime.set_embedding_provider(provider);
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
    // Plan 0250 (GAR-771): wire the default persona. When no `system_prompt` is
    // configured, Garra speaks with the warm "friendly" voice by default; set
    // `agent.persona = "neutral"` to opt out.
    runtime.set_persona_mode(match config.agent.persona {
        garraia_config::model::PersonaMode::Friendly => garraia_agents::PersonaMode::Friendly,
        garraia_config::model::PersonaMode::Neutral => garraia_agents::PersonaMode::Neutral,
    });
    if let Some(lang) = &config.agent.persona_lang {
        runtime.set_persona_lang(garraia_agents::Lang::from_code(lang));
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

    // #952: o que nao merece vetor na ingestao.
    {
        let policy = noise_policy_from_config(config);
        if policy.is_enabled() {
            info!(
                min_chars = policy.min_chars(),
                extras = config.memory.ingestion.extra_noise_phrases.len(),
                "filtro de ruido da memoria ativo: turno curto ou puramente \
                 social e gravado sem vetor (#952). Desligue em \
                 `memory.ingestion.filter_noise` se quiser embeddar tudo."
            );
        } else {
            info!("filtro de ruido da memoria desligado por config (#952)");
        }
        runtime.set_noise_policy(policy);
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

/// Sobe os adapters de hardware configurados (#1126 MQTT, #1127 Home
/// Assistant) quando houver secao `hardware.mqtt` ou
/// `hardware.home_assistant` no config.
///
/// Chamado pelo gateway **e** pela CLI (`garra chat`) — e a fonte unica do
/// wiring: resolucao de segredos, caminho do store de presenca e o fato de
/// gateway e CLI abrirem o MESMO store moram aqui, nao em copias que
/// divergem. A CLI chama via
/// `garraia_gateway::bootstrap::spawn_hardware_adapters`.
///
/// O store de presenca abre **uma vez**, compartilhado entre adapters: os
/// dois alimentam a mesma fonte de online/offline que as tools de device
/// mostram. `None` quando nao ha nenhuma secao de hardware no config, ou
/// quando o store nao abre (nenhum adapter sobe sem presenca).
///
/// O barramento de eventos (#1128) nasce aqui e os adapters publicam nele
/// o que veem; o motor de automacoes assina quando `hardware.automations`
/// esta configurado (regras do dir, auditoria em `automations.db`, teto de
/// risco do config). Automations sem nenhum adapter configurado nao sobe —
/// nenhum evento chegaria ao barramento — e o warn diz isso.
///
/// Fail-soft no boot: qualquer problema (broker malformado, `password_env`/
/// `token_env` apontando para env vazia ou inexistente, URL vetada recusada
/// pelo guard SSRF) nao derruba o processo — o registry segue com os
/// dispositivos dos adapters que subiram, e cada warn diz o que faltou.
/// `garra config check` mostra os mesmos erros de config antes do boot.
///
/// Precisa de runtime tokio (spawn dos event loops) — gateway e CLI chamam
/// de dentro de `run()` async. Sem secao `hardware.*`, retorna antes de
/// tocar tokio, seguro para testes.
pub fn spawn_hardware_adapters(
    config: &AppConfig,
    registry: Arc<DeviceRegistry>,
) -> Option<Arc<DeviceStateStore>> {
    if config.hardware.mqtt.is_none() && config.hardware.home_assistant.is_none() {
        if config.hardware.automations.is_some() {
            warn!(
                "hardware: automations configurado sem nenhum adapter (mqtt/home_assistant); \
                 o motor nao sobe porque nenhum evento chegaria ao barramento"
            );
        }
        return None;
    }

    // Presenca no mesmo padrao do memory.db: fonte unica da resolucao em
    // `AppConfig::hardware_db_path`, porque gateway e CLI abrem o mesmo
    // arquivo. O diretorio precisa existir antes de abrir o SQLite — o boot
    // da memoria cria o dele, mas o hardware pode rodar sem memoria ligada.
    let state_path = config.hardware_db_path();
    if let Some(parent) = state_path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        warn!(
            "hardware: nao consegui criar {} ({e}); nenhum adapter de hardware sobe",
            parent.display()
        );
        return None;
    }
    let state = match DeviceStateStore::abrir_em(&state_path) {
        Ok(store) => Arc::new(store),
        Err(e) => {
            warn!(
                "hardware: nao abri o store de presenca em {} ({e}); nenhum adapter de hardware sobe",
                state_path.display()
            );
            return None;
        }
    };

    // Barramento de eventos (#1128): adapters publicam o que veem e o motor
    // de automacoes assina. Publicar sem assinante custa zero (o canal
    // descarta), entao ele existe sempre que ha hardware no ar.
    let bus = Arc::new(HardwareEventBus::nova());

    // Cada adapter decide sozinho se sobe e explica o que faltou. O
    // operador pode ter os dois, um so, ou nenhum — o registry soma.
    let mut algum_no_ar = false;
    algum_no_ar |= sobe_mqtt(config, registry.clone(), state.clone(), Some(bus.clone()));
    algum_no_ar |= sobe_home_assistant(config, registry.clone(), state.clone(), Some(bus.clone()));

    // O motor de automacoes (#1128) arma por conta do config, independente
    // de quantos adapters subiram — cada falha anterior ja teve o seu warn.
    sobe_automacoes(config, bus, registry);

    algum_no_ar.then_some(state)
}

/// Sobe o adapter MQTT (#1126) quando `hardware.mqtt` esta configurado.
/// Publica presenca e estado no barramento (`bus`) que o motor de
/// automacoes (#1128) assina. Fail-soft: cada problema vira warn e devolve
/// `false` — o deploy segue no ar, sem o transporte.
fn sobe_mqtt(
    config: &AppConfig,
    registry: Arc<DeviceRegistry>,
    state: Arc<DeviceStateStore>,
    bus: Option<Arc<HardwareEventBus>>,
) -> bool {
    let Some(mqtt) = config.hardware.mqtt.as_ref() else {
        return false;
    };

    // A senha vem do env apontado por `password_env` e nunca e logada — o
    // warn cita so o NOME da env, que nao e segredo. Configurada e vazia/
    // ausente e falha de deploy, nao motivo para conectar anonimo: o broker
    // recusaria depois, com reconnect em loop e sem mensagem clara.
    let password = match &mqtt.password_env {
        Some(env_name) => match std::env::var(env_name) {
            Ok(value) if !value.is_empty() => Some(value),
            _ => {
                warn!(
                    env = env_name,
                    "hardware.mqtt: password_env aponta para env vazia ou inexistente; \
                     adapter MQTT nao sobe e o registry de dispositivos fica vazio"
                );
                return false;
            }
        },
        None => None,
    };

    let adapter_config = match MqttAdapterConfig::new(&mqtt.broker, mqtt.username.clone(), password)
    {
        Ok(cfg) => cfg,
        Err(e) => {
            warn!(
                "hardware.mqtt: {e}; adapter MQTT nao sobe e o registry de dispositivos fica vazio (veja `garra config check`)"
            );
            return false;
        }
    };

    // Client id unico por processo: gateway e `garra chat` no mesmo host
    // conectam ao mesmo broker, e dois clientes com o mesmo id se des
    // conectam em loop — o pid completa a unicidade do prefixo.
    let adapter_config =
        adapter_config.com_client_id(format!("{}-{}", mqtt.client_id_prefix, std::process::id()));

    // O handle do manager fica solto de proposito: o event loop vive pela
    // vida do processo (reconnect do rumqttc e interno), e o reload de
    // config que o pararia e trabalho da #1128.
    if let Err(e) = MqttAdapterManager::spawn(adapter_config, registry, Some(state), bus) {
        warn!("hardware.mqtt: {e}; adapter MQTT nao sobe e o registry de dispositivos fica vazio");
        return false;
    }

    info!(
        broker = %mqtt.broker,
        "hardware.mqtt no ar — descoberta via manifestos retained, presenca no store"
    );
    true
}

/// Sobe o adapter Home Assistant (#1127) quando `hardware.home_assistant`
/// esta configurado — REST (descoberta `/api/states`, leitura, servicos) +
/// WebSocket (`state_changed` → presenca + barramento #1128), tudo atras do
/// guard SSRF (`IpScope::AllowPrivate`: hub na LAN/loopback e alvo
/// legitimo). As entidades viram dispositivos com risco por dominio:
/// sensor/binary_sensor R0, light/switch/climate R1, cover R2, lock R3.
///
/// O token vem do env apontado por `token_env` (write-only: nunca logado,
/// o warn cita so o NOME da env). Fail-soft igual ao MQTT.
fn sobe_home_assistant(
    config: &AppConfig,
    registry: Arc<DeviceRegistry>,
    state: Arc<DeviceStateStore>,
    bus: Option<Arc<HardwareEventBus>>,
) -> bool {
    let Some(ha) = config.hardware.home_assistant.as_ref() else {
        return false;
    };

    let token = match std::env::var(&ha.token_env) {
        Ok(value) if !value.is_empty() => value,
        _ => {
            warn!(
                env = %ha.token_env,
                "hardware.home_assistant: token_env aponta para env vazia ou inexistente; \
                 adapter Home Assistant nao sobe e o registry de dispositivos fica vazio"
            );
            return false;
        }
    };

    let adapter_config = match HaAdapterConfig::new(&ha.url, token) {
        Ok(cfg) => cfg,
        Err(e) => {
            warn!(
                "hardware.home_assistant: {e}; adapter nao sobe e o registry de dispositivos fica vazio (veja `garra config check`)"
            );
            return false;
        }
    };

    // O handle do manager fica solto de proposito: o loop vive pela vida do
    // processo (reconexao do WebSocket e interna), e o reload de config que
    // o pararia e trabalho da #1128.
    HaAdapterManager::spawn(adapter_config, registry, Some(state), bus);
    info!(
        url = %ha.url,
        "hardware.home_assistant no ar — descoberta por /api/states, presenca por eventos state_changed"
    );
    true
}

/// Arma o motor de automacoes (#1128) quando `hardware.automations` esta
/// configurado: regras compiladas do dir (TOML/JSON), auditoria em
/// `automations.db` e o teto de risco do config. Fail-soft igual aos
/// adapters — cada problema vira warn e o boot segue sem o motor. Regra
/// quebrada nunca derruba o deploy, mas tambem nunca sobe silenciosa.
fn sobe_automacoes(config: &AppConfig, bus: Arc<HardwareEventBus>, registry: Arc<DeviceRegistry>) {
    let Some(dir) = config.automations_dir() else {
        return; // sem secao no config — o motor nao sobe (fail-closed)
    };

    // Auditoria e pre-condicao do motor: execucao sem auditoria quebraria
    // o contrato #1128, entao sem store nao ha motor. O diretorio do
    // `automations.db` e o mesmo do store de presenca, ja criado acima.
    let store_path = config.automations_db_path();
    let store = match AutomationStore::abrir_em(&store_path) {
        Ok(store) => Arc::new(store),
        Err(e) => {
            warn!(
                "hardware.automations: nao abri a auditoria em {} ({e}); motor nao sobe",
                store_path.display()
            );
            return;
        }
    };

    // Regras do dir: arquivo quebrado nomeia o arquivo no erro, e dir
    // ausente tambem e erro — o operador declarou `dir`, e "nenhuma regra
    // no ar" tem que ser dito, nao presumido.
    let specs = match carregar_automacoes(&dir) {
        Ok(specs) => specs,
        Err(e) => {
            warn!(
                "hardware.automations: {e} — regras de {}; motor nao sobe com spec quebrada",
                dir.display()
            );
            return;
        }
    };

    // Teto do config, com o default r1 do schema se ausente. `de_texto` so
    // aceita r0/r1/r2 — o `garra config check` ja recusa R3+ antes do boot.
    let teto = config
        .automations_risk_ceiling()
        .and_then(TetoRisco::de_texto)
        .unwrap_or(TetoRisco::R1);

    // O handle fica solto de proposito — o motor vive pela vida do
    // processo, igual aos event loops dos adapters.
    if let Err(e) = AutomationEngine::spawn(EngineConfig { specs, teto }, bus, registry, store) {
        warn!("hardware.automations: {e}; motor nao sobe");
        return;
    }
    info!(
        regras = %dir.display(),
        teto = teto.as_str(),
        "hardware.automations no ar — regras compiladas, motor assinando o barramento"
    );
}

/// Build MCP tools from merged config (config.yml + mcp.json).
///
/// Returns the manager, a flat list of bridged tools ready for registration,
/// and the servers that failed to connect as `(name, error)` pairs so the
/// caller can surface them (registry status, admin UI) instead of the
/// failures living only in a warn! line.
pub async fn build_mcp_tools(
    config: &AppConfig,
) -> (Arc<McpManager>, Vec<Box<dyn Tool>>, Vec<(String, String)>) {
    let loader = match garraia_config::ConfigLoader::new() {
        Ok(l) => l,
        Err(e) => {
            warn!("failed to create config loader for MCP: {e}");
            return (Arc::new(McpManager::new()), Vec::new(), Vec::new());
        }
    };

    let mcp_configs = loader.merged_mcp_config(config);
    if mcp_configs.is_empty() {
        // Explicit log: "nothing configured" used to be indistinguishable
        // from "config file in another directory was silently ignored".
        info!(
            config_dir = %loader.config_dir().display(),
            config_yml_mcp = config.mcp.len(),
            "no MCP servers configured (checked mcp.json and the 'mcp:' section of config.yml in this directory)"
        );
        return (Arc::new(McpManager::new()), Vec::new(), Vec::new());
    }

    let manager = Arc::new(McpManager::new());
    let mut all_tools: Vec<Box<dyn Tool>> = Vec::new();
    let mut failures: Vec<(String, String)> = Vec::new();

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
                failures.push((name.clone(), e.to_string()));
                // Boot failures used to be terminal: the server never entered
                // `connections`, so the health monitor could not see it and
                // only a manual admin restart recovered it. Queue it for the
                // same backoff-driven retry as a crashed connection.
                if server_config.transport == "stdio" {
                    manager
                        .register_pending_stdio(
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
                        .await;
                }
            }
        }
    }

    (manager, all_tools, failures)
}

/// Build a voice handler for Telegram that processes voice messages.
/// Constroi o provider de embeddings declarado na config, ja envelopado no
/// [`ResilientEmbeddingProvider`] (#962, #961).
///
/// Extraido de [`build_agent_runtime`] para que a CLI possa pedir **o mesmo**
/// provider que o gateway usa sem subir um runtime inteiro (e sem abrir uma
/// segunda conexao no `memory.db`) — `garra memory reindex` (#953) grava
/// vetores que o recall do gateway vai ler, entao os dois lados precisam
/// concordar na resolucao da chave, no timeout proprio e na dimensao validada.
/// Construir um segundo provider por fora era exatamente a divergencia
/// silenciosa que este projeto ja pagou caro.
///
/// Devolve `None` quando `memory.embedding_provider` nao aponta para nenhuma
/// entrada de `embeddings:`, ou quando o provider apontado nao pode ser
/// construido (sem credencial contra endpoint oficial, tipo desconhecido).
/// A politica de ruido da ingestao (#952), a partir da config.
///
/// Publica pela mesma razao que [`build_embedding_provider`]: o `garra memory
/// reindex` precisa da **mesma** politica que a ingestao usou. Com politicas
/// divergentes, o reindex reembeddaria exatamente as entradas que a ingestao
/// acabou de pular — e o operador pagaria provider para desfazer o filtro.
pub fn noise_policy_from_config(config: &AppConfig) -> NoisePolicy {
    let i = &config.memory.ingestion;
    NoisePolicy::new(i.filter_noise, i.min_chars, &i.extra_noise_phrases)
}

pub fn build_embedding_provider(config: &AppConfig) -> Option<Arc<dyn EmbeddingProvider>> {
    let embed_name = config.memory.embedding_provider.as_ref()?;
    let embed_config = config.embeddings.get(embed_name)?;

    let embed_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            config.timeouts.embeddings.default_secs,
        ))
        .build()
        .unwrap_or_default();

    let built: Option<Arc<dyn EmbeddingProvider>> = match embed_config.provider.as_str() {
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
                Some(Arc::new(
                    CohereEmbeddingProvider::new(
                        key,
                        embed_config.model.clone(),
                        embed_config.base_url.clone(),
                    )
                    .with_client(embed_client.clone()),
                ))
            } else {
                warn!("skipping cohere embedding provider: no API key");
                None
            }
        }

        // =====================================
        // OLLAMA (ADICIONE ESTE BLOCO)
        // =====================================
        "ollama" => Some(Arc::new(
            OllamaEmbeddingProvider::new(embed_config.model.clone(), embed_config.base_url.clone())
                .with_client(embed_client.clone()),
        )),

        // =====================================
        // OPENAI-COMPATIBLE (LM Studio, OpenAI, etc.)
        // =====================================
        "openai" => {
            // Antes daqui saia o literal "no-key" para qualquer
            // endpoint. Contra a OpenAI oficial isso e um 401 em
            // toda chamada — e o erro era engolido logo adiante
            // (#948), entao a memoria simplesmente parava de ser
            // indexada sem ninguem saber. E a chave nem era
            // procurada no ambiente, so no arquivo de config.
            let api_key = resolve_api_key(
                embed_config.api_key.as_deref(),
                KEY_ENV_VAR_OPENAI,
                KEY_ENV_VAR_OPENAI,
            );
            let self_hosted = is_self_hosted_openai_endpoint(embed_config.base_url.as_deref());

            match (api_key, self_hosted) {
                (Some(key), _) => Some(Arc::new(
                    OpenAiEmbeddingProvider::new(
                        key,
                        embed_config.model.clone(),
                        embed_config.base_url.clone(),
                    )
                    .with_client(embed_client.clone()),
                )),
                // LM Studio, vLLM, gateway interno: nao pedem
                // credencial, e mandar um bearer vazio faz alguns
                // recusarem. O provider omite o header nesse caso.
                (None, true) => {
                    info!(
                        "openai embedding provider '{embed_name}': endpoint \
                         proprio sem credencial configurada, seguindo sem \
                         autenticacao"
                    );
                    Some(Arc::new(
                        OpenAiEmbeddingProvider::new(
                            String::new(),
                            embed_config.model.clone(),
                            embed_config.base_url.clone(),
                        )
                        .with_client(embed_client.clone()),
                    ))
                }
                (None, false) => {
                    warn!(
                        "skipping openai embedding provider '{embed_name}': \
                         sem chave no config, no cofre nem no ambiente, e o \
                         endpoint e o oficial da OpenAI — subir assim daria \
                         401 em toda chamada de embedding"
                    );
                    None
                }
            }
        }

        // =====================================
        // UNKNOWN
        // =====================================
        other => {
            warn!("unknown embedding provider type: {other}");
            None
        }
    };

    let inner = built?;
    let provider_id = inner.provider_id().to_string();
    let model = inner.model().to_string();

    // Toda saida de embeddings passa a ter retry com backoff
    // e validacao de dimensao (#962, #961) — um envelope so
    // para os tres providers, em vez de tres copias da mesma
    // logica.
    let resilient =
        ResilientEmbeddingProvider::new(inner).with_expected_dimensions(embed_config.dimensions);

    match embed_config.dimensions {
        Some(dims) => info!(
            "configured {provider_id} embedding provider: {embed_name} \
             (modelo {model}, {dims} dimensoes validadas)"
        ),
        None => info!(
            "configured {provider_id} embedding provider: {embed_name} \
             (modelo {model}; sem `dimensions` na config, o vetor \
             devolvido nao e validado — ver #961)"
        ),
    }

    Some(Arc::new(resilient))
}

/// Qual backend de busca registrar (#1034).
///
/// Escolha explicita ganha e, se faltar o que ela precisa, ninguem e
/// registrado: cair para o outro backend em silencio seria surpresa para quem
/// configurou um de proposito. Sem escolha, Brave com chave (como sempre foi)
/// e, so entao, SearXNG com URL.
pub(crate) fn select_web_search_backend(
    choice: Option<garraia_config::model::WebSearchBackend>,
    brave_key: Option<String>,
    searxng_url: Option<String>,
) -> Option<garraia_agents::tools::web_search_tool::SearchBackend> {
    use garraia_agents::tools::web_search_tool::SearchBackend;
    use garraia_config::model::WebSearchBackend;

    let brave = |api_key: String| SearchBackend::Brave { api_key };
    let searxng = |base_url: String| SearchBackend::Searxng { base_url };
    match choice {
        Some(WebSearchBackend::Brave) => brave_key.map(brave),
        Some(WebSearchBackend::Searxng) => searxng_url.map(searxng),
        None => brave_key.map(brave).or_else(|| searxng_url.map(searxng)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #1034: a regra de escolha do backend de busca, sem subir gateway.
    #[test]
    fn web_search_backend_selection() {
        use garraia_agents::tools::web_search_tool::SearchBackend;
        use garraia_config::model::WebSearchBackend;

        let k = || Some("brave-key".to_string());
        let u = || Some("http://127.0.0.1:8081".to_string());

        // Sem secao: Brave com chave, como antes; sem chave, SearXNG com URL;
        // sem nada, nada.
        assert!(matches!(
            select_web_search_backend(None, k(), u()),
            Some(SearchBackend::Brave { .. })
        ));
        assert!(matches!(
            select_web_search_backend(None, None, u()),
            Some(SearchBackend::Searxng { ref base_url }) if base_url == "http://127.0.0.1:8081"
        ));
        assert!(select_web_search_backend(None, None, None).is_none());

        // Explicito ganha mesmo com o outro disponivel...
        assert!(matches!(
            select_web_search_backend(Some(WebSearchBackend::Searxng), k(), u()),
            Some(SearchBackend::Searxng { .. })
        ));
        assert!(matches!(
            select_web_search_backend(Some(WebSearchBackend::Brave), k(), u()),
            Some(SearchBackend::Brave { .. })
        ));
        // ...e sem o que precisa nao cai para o outro.
        assert!(select_web_search_backend(Some(WebSearchBackend::Searxng), k(), None).is_none());
        assert!(select_web_search_backend(Some(WebSearchBackend::Brave), None, u()).is_none());
    }

    /// #1033 / #1035: as ferramentas de exploracao entram no runtime sem
    /// depender de provider. `code_review` precisa de um LLM por dentro e,
    /// com config vazia, fica de fora — sem derrubar o boot e sem aparecer
    /// na lista que o modelo recebe.
    #[test]
    fn build_agent_runtime_registers_exploration_tools() {
        let runtime = build_agent_runtime(&AppConfig::default());
        let names = runtime.tool_names();
        for expected in [
            "bash",
            "file_read",
            "file_write",
            "web_fetch",
            "list_dir",
            "repo_search",
            "run_tests",
        ] {
            assert!(
                names.iter().any(|n| n == expected),
                "{expected} ausente em {names:?}"
            );
        }
        assert!(
            !names.iter().any(|n| n == "code_review"),
            "sem provider default, code_review nao pode ter sido registrada: {names:?}"
        );
    }

    /// O literal "no-key" que existia aqui tratava LM Studio e a OpenAI
    /// oficial igual. Contra a oficial isso e 401 em toda chamada; contra o
    /// endpoint proprio, seguir sem credencial e o comportamento certo.
    #[test]
    fn distinguishes_self_hosted_openai_endpoints_from_the_official_one() {
        // Sem base_url o provider assume api.openai.com — precisa de chave.
        assert!(!is_self_hosted_openai_endpoint(None));
        assert!(!is_self_hosted_openai_endpoint(Some(
            "https://api.openai.com"
        )));
        assert!(!is_self_hosted_openai_endpoint(Some(
            "https://api.openai.com/v1"
        )));

        // Endpoints proprios: nao pedem credencial.
        assert!(is_self_hosted_openai_endpoint(Some(
            "http://localhost:1234/v1"
        )));
        assert!(is_self_hosted_openai_endpoint(Some(
            "http://127.0.0.1:8000"
        )));
        assert!(is_self_hosted_openai_endpoint(Some(
            "https://llm.interno.empresa.com/v1"
        )));
    }

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

    // ─── #952: a politica de ruido, config <-> agents ─────────────────────

    /// `garraia-agents` nao depende de `garraia-config` (de proposito: a
    /// politica chega la como dado puro), entao o piso de caracteres e a
    /// faixa aceita existem escritos **duas vezes**. Este crate e o unico
    /// que ve os dois lados, e este teste e o que impede os dois numeros de
    /// divergirem em silencio — o sintoma seria um `config check` aceitando
    /// um valor que a politica trata de outro jeito.
    #[test]
    fn os_dois_lados_da_politica_de_ruido_concordam_nos_numeros() {
        assert_eq!(
            garraia_agents::DEFAULT_MIN_CHARS,
            garraia_config::model::IngestionConfig::default().min_chars,
            "default do piso divergiu entre garraia-agents e garraia-config"
        );
        assert_eq!(
            garraia_agents::MIN_CHARS_MAX,
            garraia_config::model::INGESTION_MIN_CHARS_MAX,
            "teto do piso divergiu entre garraia-agents e garraia-config"
        );
    }

    /// A config default tem que produzir a politica default. Se o bootstrap
    /// deixasse de ler alguma chave, o gateway rodaria com uma politica e o
    /// `garra memory reindex` com outra — e o reindex desfaria o filtro.
    #[test]
    fn config_default_produz_a_politica_default() {
        let config = AppConfig::default();
        assert_eq!(
            noise_policy_from_config(&config),
            garraia_agents::NoisePolicy::default()
        );
    }

    #[test]
    fn filter_noise_false_desliga_a_politica() {
        let mut config = AppConfig::default();
        config.memory.ingestion.filter_noise = false;
        let policy = noise_policy_from_config(&config);
        assert!(!policy.is_enabled());
        assert!(!policy.is_noise("oi"));
    }

    #[test]
    fn extras_da_config_chegam_na_politica() {
        let mut config = AppConfig::default();
        config.memory.ingestion.extra_noise_phrases = vec!["salve familia".to_string()];
        let policy = noise_policy_from_config(&config);
        assert!(policy.is_noise("Salve, familia!"));
        assert!(
            policy.is_noise("bom dia"),
            "a lista padrao continua valendo"
        );
    }
}
