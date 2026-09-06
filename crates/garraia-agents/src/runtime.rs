// Plan 0049: `mod tests` sits mid-file; trailing private helpers
// (`estimate_tokens`, `trim_messages_to_budget`, `extract_text`) are used by
// the runtime public API above. Moving them before the tests would disrupt
// git blame on a 1.9kloc file — inner allow at module scope.
#![allow(clippy::items_after_test_module)]

use std::pin::Pin;
use std::sync::{Arc, RwLock};

use futures::future::join_all;
use futures::{Stream, StreamExt};
use garraia_common::{Error, Result, metrics};
use garraia_db::{MemoryEntry, MemoryProvider, MemoryRole, NewMemoryEntry, RecallQuery};
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::{debug, info, instrument, warn};

use crate::context_policy::ContextPolicy;
use crate::embeddings::EmbeddingProvider;
use crate::execution_budget::ExecutionBudget;
use crate::memory_extractor::LlmMemoryExtractor;
use crate::provider_resilience::ResilienceManager;
use crate::providers::{
    ChatMessage, ChatRole, ContentBlock, LlmProvider, LlmRequest, LlmResponse, MessagePart,
    StreamEvent, ToolDefinition,
};
use crate::tools::{Tool, ToolContext, ToolOutput};
use crate::turn_events::{TurnSink, summarize_tool_input, summarize_tool_output};

/// Where a registered tool came from. Issue #924: without this the runtime
/// cannot tell a native tool from an MCP one, so it cannot replace just the
/// MCP half when a server reconnects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolSource {
    Native,
    Mcp { server: String },
}

struct RegisteredTool {
    tool: Arc<dyn Tool>,
    source: ToolSource,
}

/// What one `replace_mcp_tools` call changed. Returned so callers can log a
/// real delta instead of "sync ran".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolSyncDelta {
    pub removed: usize,
    pub added: usize,
}

/// One row of [`AgentRuntime::tool_inventory`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolInventoryEntry {
    pub name: String,
    pub description: String,
    /// `"native"` or `"mcp"`.
    pub source: String,
    /// The MCP server this tool came from, when `source == "mcp"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// GAR-187: Detect if the user is approving a pending tool confirmation.
///
/// Returns `true` when:
/// 1. The recent conversation history contains a `[CONFIRM_REQUIRED]` marker
///    (emitted by `BashTool` or other tools that require human-in-the-loop), AND
/// 2. `user_text` is a simple approval word ("sim", "yes", "confirmar", etc.).
///
/// Only scans the last 6 messages to avoid false positives from old confirmations.
fn detect_confirmation_approval(history: &[ChatMessage], user_text: &str) -> bool {
    let text = user_text.trim().to_lowercase();
    let approval_words = [
        "sim",
        "yes",
        "confirmar",
        "confirma",
        "proceed",
        "ok",
        "approve",
    ];
    let is_approval = approval_words.iter().any(|w| text == *w);
    if !is_approval {
        return false;
    }

    // Check recent history for the [CONFIRM_REQUIRED] marker
    history.iter().rev().take(6).any(|msg| {
        let contains_marker = |s: &str| s.contains("[CONFIRM_REQUIRED]");
        match &msg.content {
            MessagePart::Text(t) => contains_marker(t),
            MessagePart::Parts(parts) => parts.iter().any(|p| match p {
                ContentBlock::ToolResult { content, .. } => contains_marker(content),
                ContentBlock::Text { text } => contains_marker(text),
                _ => false,
            }),
        }
    })
}

/// GAR-210: Returns true for errors that warrant a retry or provider fallback.
/// Detects rate-limit (429) and transient server errors (502/503/529).
fn is_retryable_error(err: &Error) -> bool {
    let msg = err.to_string().to_lowercase();
    msg.contains("429")
        || msg.contains("rate limit")
        || msg.contains("rate_limit")
        || msg.contains("too many requests")
        || msg.contains("status=502")
        || msg.contains("status=503")
        || msg.contains("status=529")
        || msg.contains("upstream")
}

/// Resolve provider ID from model override.
/// Models like "openrouter/auto", "openai/gpt-4o" have the provider as prefix.
fn resolve_provider_from_model(model: &str) -> Option<String> {
    let model = model.trim();
    if model.is_empty() {
        return None;
    }

    // Check for provider prefix (e.g., "openrouter/auto", "anthropic/claude-3")
    if let Some((provider, _)) = model.split_once('/') {
        let provider = provider.to_lowercase();
        // Map common provider names to registered provider IDs
        match provider.as_str() {
            "openrouter" => Some("openrouter".to_string()),
            "openai" => Some("openai".to_string()),
            "anthropic" => Some("anthropic".to_string()),
            "ollama" => Some("ollama".to_string()),
            "deepseek" => Some("deepseek".to_string()),
            "mistral" => Some("mistral".to_string()),
            "gemini" => Some("gemini".to_string()),
            "cohere" => Some("cohere".to_string()),
            "jais" => Some("jais".to_string()),
            "qwen" => Some("qwen".to_string()),
            "yi" => Some("yi".to_string()),
            "moonshot" | "kimi" => Some("moonshot".to_string()),
            "minimax" => Some("minimax".to_string()),
            "sansa" => Some("sansa".to_string()),
            "falcon" => Some("falcon".to_string()),
            _ => Some(provider), // Use as-is for unknown providers
        }
    } else {
        None
    }
}

/// Manages agent sessions, tool execution, and LLM provider routing.
pub struct AgentRuntime {
    providers: RwLock<Vec<Arc<dyn LlmProvider>>>,
    default_provider: RwLock<Option<String>>,
    memory: Option<Arc<dyn MemoryProvider>>,
    embeddings: Option<Arc<dyn EmbeddingProvider>>,
    /// Issue #924: era `Vec<Box<dyn Tool>>` e congelava no boot, quando o
    /// `AgentRuntime` entra num `Arc`. As tools MCP so eram registradas ali;
    /// se o connect do boot falhasse e o health monitor reconectasse depois,
    /// `list_servers()` passava a reportar o servidor conectado com N tools e
    /// o runtime seguia sem nenhuma delas — inclusive para o
    /// `tool_definitions()` que alimenta o tool-calling do LLM.
    ///
    /// `RwLock` da mutabilidade interior (registro pos-`Arc`), e `Arc<dyn Tool>`
    /// e o que permite `find_tool` devolver algo proprio em vez de um
    /// emprestimo preso ao guard.
    tools: RwLock<Vec<RegisteredTool>>,
    system_prompt: Option<String>,
    /// Plan 0250 (GAR-771): default persona used when `system_prompt` is unset.
    /// `Friendly` gives Garra a warm default voice; `Neutral` restores the
    /// pre-0250 behavior (no default system prompt).
    persona_mode: crate::persona::PersonaMode,
    /// Plan 0250: language for the default persona copy (PT-BR default).
    persona_lang: crate::persona::Lang,
    max_tokens: Option<u32>,
    max_context_tokens: Option<usize>,
    max_tool_calls: Option<usize>,
    memory_extractor: LlmMemoryExtractor,
    /// GAR-210: Circuit breaker + model cache manager.
    resilience: Arc<ResilienceManager>,
    /// GAR-210: Ordered fallback provider IDs (tried when primary fails with 429/5xx).
    fallback_providers_list: RwLock<Vec<String>>,
    /// GAR-208: Sliding window + summarization policy.
    context_policy: ContextPolicy,
    /// Model to use when tools are available and the default model may not support function calling.
    /// Overrides model_override for any request that has tools registered.
    tools_model: RwLock<Option<String>>,
    /// #952: o que nao merece vetor. Ver `crate::memory_noise`.
    noise_policy: crate::memory_noise::NoisePolicy,
}

impl AgentRuntime {
    pub fn new() -> Self {
        Self {
            providers: RwLock::new(Vec::new()),
            default_provider: RwLock::new(None),
            memory: None,
            embeddings: None,
            tools: RwLock::new(Vec::new()),
            system_prompt: None,
            persona_mode: crate::persona::PersonaMode::default(),
            persona_lang: crate::persona::Lang::default(),
            max_tokens: None,
            max_context_tokens: None,
            max_tool_calls: None,
            memory_extractor: LlmMemoryExtractor::new(),
            resilience: Arc::new(ResilienceManager::new()),
            fallback_providers_list: RwLock::new(Vec::new()),
            context_policy: ContextPolicy::default(),
            tools_model: RwLock::new(None),
            noise_policy: crate::memory_noise::NoisePolicy::default(),
        }
    }

    /// Set the model to use when tools are available (overrides model_override for tool-capable requests).
    pub fn set_tools_model(&self, model: Option<String>) {
        *self.tools_model.write().unwrap() = model;
    }

    /// If `tools_model` is configured and there are tools registered, re-resolve
    /// (provider, model) so that tool-capable requests use a model that supports
    /// function calling (e.g. when the default is `openrouter/free`).
    /// Returns the original pair unchanged when no override applies.
    fn apply_tools_model_override(
        &self,
        provider: Arc<dyn LlmProvider>,
        effective_model: String,
        tool_count: usize,
    ) -> (Arc<dyn LlmProvider>, String) {
        if tool_count == 0 {
            return (provider, effective_model);
        }
        let tm = self.tools_model.read().unwrap().clone();
        let Some(tools_model) = tm.filter(|s| !s.is_empty()) else {
            return (provider, effective_model);
        };
        // Re-resolve provider: try the prefix (e.g. "google"), then "openrouter", then keep original.
        let new_provider = resolve_provider_from_model(&tools_model)
            .and_then(|pid| self.get_provider(&pid))
            .or_else(|| self.get_provider("openrouter"))
            .unwrap_or_else(|| provider.clone());
        info!(
            "tools_model override: '{}' → '{}' (provider: {})",
            effective_model,
            tools_model,
            new_provider.provider_id()
        );
        (new_provider, tools_model)
    }

    /// GAR-208: Set the context window / summarization policy.
    pub fn set_context_policy(&mut self, policy: ContextPolicy) {
        self.context_policy = policy;
    }

    /// GAR-208: Return a reference to the current context policy.
    pub fn context_policy(&self) -> &ContextPolicy {
        &self.context_policy
    }

    /// #952: define o que nao merece vetor na ingestao.
    pub fn set_noise_policy(&mut self, policy: crate::memory_noise::NoisePolicy) {
        self.noise_policy = policy;
    }

    /// #952: a politica em vigor. A CLI precisa dela para reindexar com o
    /// **mesmo** criterio da ingestao — sem isso o `garra memory reindex`
    /// reembeddaria exatamente o ruido que a ingestao acabou de pular.
    pub fn noise_policy(&self) -> &crate::memory_noise::NoisePolicy {
        &self.noise_policy
    }

    /// GAR-210: Set the ordered fallback provider list (tried on 429/5xx).
    pub fn set_fallback_providers(&self, providers: Vec<String>) {
        *self.fallback_providers_list.write().unwrap() = providers;
    }

    /// GAR-210: Return the configured fallback provider IDs.
    pub fn fallback_providers(&self) -> Vec<String> {
        self.fallback_providers_list.read().unwrap().clone()
    }

    pub fn system_prompt(&self) -> Option<&str> {
        self.system_prompt.as_deref()
    }

    pub fn set_system_prompt(&mut self, prompt: String) {
        self.system_prompt = Some(prompt);
    }

    /// Plan 0250 (GAR-771): choose the default persona voice used when no
    /// explicit `system_prompt` is set.
    pub fn set_persona_mode(&mut self, mode: crate::persona::PersonaMode) {
        self.persona_mode = mode;
    }

    /// Plan 0250: set the language for the default persona copy.
    pub fn set_persona_lang(&mut self, lang: crate::persona::Lang) {
        self.persona_lang = lang;
    }

    /// Plan 0250: resolve the base system prompt, applying the default persona
    /// fallback when no explicit prompt is configured. An explicit
    /// (non-empty) prompt always wins; `PersonaMode::Neutral` yields `None`
    /// (pre-0250 behavior).
    fn base_system_prompt(&self, explicit: Option<&str>) -> Option<String> {
        crate::persona::resolve_system_prompt(explicit, self.persona_mode, self.persona_lang)
    }

    pub fn set_max_tokens(&mut self, max_tokens: u32) {
        self.max_tokens = Some(max_tokens);
    }

    pub fn set_max_context_tokens(&mut self, max_context_tokens: usize) {
        self.max_context_tokens = Some(max_context_tokens);
    }

    pub fn set_max_tool_calls(&mut self, max_tool_calls: usize) {
        self.max_tool_calls = Some(max_tool_calls);
    }

    pub fn register_provider(&self, provider: Arc<dyn LlmProvider>) {
        let id = provider.provider_id().to_string();
        info!("registered LLM provider: {}", id);
        {
            let mut default = self.default_provider.write().unwrap();
            if default.is_none() {
                *default = Some(id);
            }
        }
        self.providers.write().unwrap().push(provider);
    }

    /// GAR-208: Return all registered providers (cloned Arc handles).
    pub fn list_providers(&self) -> Vec<Arc<dyn LlmProvider>> {
        self.providers.read().unwrap().clone()
    }

    pub fn get_provider(&self, id: &str) -> Option<Arc<dyn LlmProvider>> {
        self.providers
            .read()
            .unwrap()
            .iter()
            .find(|p| p.provider_id() == id)
            .cloned()
    }

    pub fn default_provider(&self) -> Option<Arc<dyn LlmProvider>> {
        let default_id = self.default_provider.read().unwrap().clone();
        default_id.and_then(|id| self.get_provider(&id))
    }

    /// Return the IDs of all registered providers.
    pub fn provider_ids(&self) -> Vec<String> {
        self.providers
            .read()
            .unwrap()
            .iter()
            .map(|p| p.provider_id().to_string())
            .collect()
    }

    /// Set the default provider by ID. Returns `true` if the provider exists.
    pub fn set_default_provider_id(&self, id: &str) -> bool {
        let exists = self
            .providers
            .read()
            .unwrap()
            .iter()
            .any(|p| p.provider_id() == id);
        if exists {
            *self.default_provider.write().unwrap() = Some(id.to_string());
        }
        exists
    }

    /// Return the current default provider ID.
    pub fn default_provider_id(&self) -> Option<String> {
        self.default_provider.read().unwrap().clone()
    }

    pub fn set_memory_provider(&mut self, memory: Arc<dyn MemoryProvider>) {
        self.memory = Some(memory);
        info!("memory provider attached to agent runtime");
    }

    pub fn has_memory_provider(&self) -> bool {
        self.memory.is_some()
    }

    pub fn memory_provider(&self) -> Option<Arc<dyn MemoryProvider>> {
        self.memory.clone()
    }

    /// Return (name, description) pairs for all registered tools.
    pub fn list_tool_info(&self) -> Vec<(String, String)> {
        self.tools
            .read()
            .unwrap()
            .iter()
            .map(|r| (r.tool.name().to_string(), r.tool.description().to_string()))
            .collect()
    }

    /// O provider de embeddings ativo, se houver.
    ///
    /// Existe para o boot poder chamar `health_check()` (#951) — que ate
    /// aqui era codigo morto, declarado no trait e nunca invocado — e para a
    /// reindexacao da CLI (#953) reusar exatamente o mesmo provider que o
    /// runtime usa, em vez de construir um segundo por fora.
    pub fn embedding_provider(&self) -> Option<Arc<dyn EmbeddingProvider>> {
        self.embeddings.clone()
    }

    pub fn set_embedding_provider(&mut self, embeddings: Arc<dyn EmbeddingProvider>) {
        self.embeddings = Some(embeddings);
        info!("embedding provider attached to agent runtime");
    }

    pub fn has_embedding_provider(&self) -> bool {
        self.embeddings.is_some()
    }

    pub async fn on_session_start(
        &self,
        session_id: &str,
        continuity_key: Option<&str>,
    ) -> Result<()> {
        self.remember_system_event(
            session_id,
            continuity_key,
            "session_started",
            "Session started",
        )
        .await
    }

    pub async fn on_session_end(
        &self,
        session_id: &str,
        continuity_key: Option<&str>,
    ) -> Result<()> {
        self.remember_system_event(session_id, continuity_key, "session_ended", "Session ended")
            .await
    }

    pub async fn remember_turn(
        &self,
        session_id: &str,
        continuity_key: Option<&str>,
        user_id: Option<&str>,
        user_input: &str,
        assistant_output: &str,
    ) -> Result<()> {
        let Some(memory) = &self.memory else {
            return Ok(());
        };
        // Skip storing empty turns — the memory store rejects blank content.
        if user_input.trim().is_empty() && assistant_output.trim().is_empty() {
            return Ok(());
        }

        if !user_input.trim().is_empty() {
            let user_embedding = self.embed_turn_unless_noise(user_input, "user").await;
            let user_embedding_model = self.embedding_model_for(&user_embedding);
            memory
                .remember(NewMemoryEntry {
                    tenant_id: "default".to_string(),
                    session_id: session_id.to_string(),
                    channel_id: None,
                    user_id: user_id.map(|s| s.to_string()),
                    continuity_key: continuity_key.map(|s| s.to_string()),
                    role: MemoryRole::User,
                    content: user_input.to_string(),
                    embedding: user_embedding,
                    embedding_model: user_embedding_model,
                    metadata: serde_json::json!({ "kind": "turn_user" }),
                })
                .await?;
        }

        if !assistant_output.trim().is_empty() {
            let assistant_embedding = self
                .embed_turn_unless_noise(assistant_output, "assistant")
                .await;
            let assistant_embedding_model = self.embedding_model_for(&assistant_embedding);
            memory
                .remember(NewMemoryEntry {
                    tenant_id: "default".to_string(),
                    session_id: session_id.to_string(),
                    channel_id: None,
                    user_id: user_id.map(|s| s.to_string()),
                    continuity_key: continuity_key.map(|s| s.to_string()),
                    role: MemoryRole::Assistant,
                    content: assistant_output.to_string(),
                    embedding: assistant_embedding,
                    embedding_model: assistant_embedding_model,
                    metadata: serde_json::json!({ "kind": "turn_assistant" }),
                })
                .await?;
        }

        Ok(())
    }

    pub async fn recall_context(
        &self,
        query_text: &str,
        session_id: Option<&str>,
        continuity_key: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let Some(memory) = &self.memory else {
            return Ok(Vec::new());
        };

        // #957: mede o recall **inteiro** — o embedding da consulta mais a
        // busca —, porque e isso que o usuario espera. Medir so a busca
        // esconderia o caso que mais dói: o provider de embeddings lento
        // fazendo o recall demorar antes mesmo de o banco ser tocado.
        let inicio = std::time::Instant::now();

        let query_embedding = self.embed_query(query_text).await;
        // O filtro por modelo só faz sentido acompanhando um embedding (#954):
        // sem vetor de consulta, o recall é textual e não deve ser estreitado.
        let embedding_model = query_embedding
            .is_some()
            .then(|| self.embedding_model())
            .flatten();

        let resultado = memory
            .recall(RecallQuery {
                tenant_id: None,
                query_text: Some(query_text.to_string()),
                query_embedding,
                embedding_model,
                session_id: session_id.map(|s| s.to_string()),
                continuity_key: continuity_key.map(|s| s.to_string()),
                limit,
            })
            .await;

        // Medido tambem quando falha: um recall que morre em 30s de timeout e
        // exatamente a latencia que o operador precisa ver. Contar so o
        // sucesso faria o painel melhorar quando o sistema piora.
        metrics::record_recall_latency(inicio.elapsed().as_secs_f64());

        resultado
    }

    /// Register a natively-built tool. Takes `&self` since #924 — the runtime
    /// is behind an `Arc` by the time some tools exist.
    pub fn register_tool(&self, tool: Box<dyn Tool>) {
        info!("registered tool: {}", tool.name());
        self.tools.write().unwrap().push(RegisteredTool {
            tool: Arc::from(tool),
            source: ToolSource::Native,
        });
    }

    /// Replace every tool sourced from `server` with `tools`.
    ///
    /// Idempotent by construction: calling it twice with the same inventory
    /// leaves the same list, which is what makes it safe to run on every
    /// health-monitor tick. A server that disconnected and came back with a
    /// smaller tool list shrinks correctly instead of accumulating stale
    /// entries — `find_tool` is a linear scan where duplicates would silently
    /// shadow each other.
    pub fn replace_mcp_tools(&self, server: &str, tools: Vec<Box<dyn Tool>>) -> ToolSyncDelta {
        let mut guard = self.tools.write().unwrap();
        let before = guard.len();
        guard.retain(|r| !matches!(&r.source, ToolSource::Mcp { server: s } if s == server));
        let removed = before - guard.len();
        let added = tools.len();
        for tool in tools {
            guard.push(RegisteredTool {
                tool: Arc::from(tool),
                source: ToolSource::Mcp {
                    server: server.to_string(),
                },
            });
        }
        ToolSyncDelta { removed, added }
    }

    /// Pull the MCP half of the tool list from the manager and make the
    /// runtime match it.
    ///
    /// Issue #924: this is the one function that closes the gap. Before it,
    /// MCP tools reached the runtime exactly once — in `Server::run`, before
    /// the runtime went into an `Arc` — so a server that connected late (or
    /// reconnected, or was added through the admin API) had live tools that
    /// `list_servers()` reported and `tool_definitions()` did not, which means
    /// the LLM could not call them. Calling this from the boot path, the health
    /// monitor and the admin handlers keeps all three honest.
    ///
    /// Safe to call on every tick: `replace_mcp_tools` is idempotent per
    /// server, and servers absent from the manager keep whatever they had —
    /// a read failure must not silently strip working tools.
    #[cfg(feature = "mcp")]
    pub async fn sync_mcp_tools(&self, manager: &Arc<crate::mcp::McpManager>) -> usize {
        let by_server = manager.tools_by_server().await;
        let mut total = 0;
        for (server, tools) in by_server {
            let count = tools.len();
            let delta = self.replace_mcp_tools(&server, tools);
            total += count;
            if delta.removed != delta.added {
                info!(
                    server = %server,
                    removed = delta.removed,
                    added = delta.added,
                    "MCP tool inventory changed in AgentRuntime"
                );
            }
        }
        total
    }

    /// GAR-159: List names of all registered tools (for API endpoints and diagnostics).
    pub fn tool_names(&self) -> Vec<String> {
        self.tools
            .read()
            .unwrap()
            .iter()
            .map(|r| r.tool.name().to_string())
            .collect()
    }

    /// Issue #924: name + origin for every registered tool, so an API can say
    /// which tools are native and which came from which MCP server instead of
    /// reporting a bare count that disagrees with `list_servers()`.
    pub fn tool_inventory(&self) -> Vec<ToolInventoryEntry> {
        self.tools
            .read()
            .unwrap()
            .iter()
            .map(|r| ToolInventoryEntry {
                name: r.tool.name().to_string(),
                description: r.tool.description().to_string(),
                source: match &r.source {
                    ToolSource::Native => "native".to_string(),
                    ToolSource::Mcp { .. } => "mcp".to_string(),
                },
                server: match &r.source {
                    ToolSource::Native => None,
                    ToolSource::Mcp { server } => Some(server.clone()),
                },
            })
            .collect()
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .read()
            .unwrap()
            .iter()
            .map(|r| ToolDefinition {
                name: r.tool.name().to_string(),
                description: r.tool.description().to_string(),
                input_schema: r.tool.input_schema(),
            })
            .collect()
    }

    fn find_tool(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools
            .read()
            .unwrap()
            .iter()
            .find(|r| r.tool.name() == name)
            .map(|r| Arc::clone(&r.tool))
    }

    /// Run the full conversation loop: recall context, call LLM, execute tools, return response.
    #[instrument(skip_all, fields(session_id = %session_id))]
    pub async fn process_message(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
    ) -> Result<String> {
        self.process_message_with_context(session_id, user_text, conversation_history, None, None)
            .await
    }

    /// Same as `process_message` but includes continuity/user context for shared memory.
    #[instrument(skip_all, fields(session_id = %session_id, has_user_id = user_id.is_some()))]
    pub async fn process_message_with_context(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
        continuity_key: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<String> {
        self.process_message_with_agent_config(
            session_id,
            user_text,
            conversation_history,
            continuity_key,
            user_id,
            None,
            None,
            None,
            None,
        )
        .await
    }

    /// Process a scheduled heartbeat message. Tools receive `is_heartbeat = true`
    /// so that recursive scheduling is blocked.
    #[instrument(skip_all, fields(session_id = %session_id, has_user_id = user_id.is_some()))]
    pub async fn process_heartbeat(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
        continuity_key: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<String> {
        self.process_message_impl(
            session_id,
            user_text,
            conversation_history,
            continuity_key,
            user_id,
            true,
        )
        .await
    }

    /// Process a message with explicit agent config overrides (for multi-agent routing).
    #[allow(clippy::too_many_arguments)]
    #[instrument(skip_all, fields(session_id = %session_id, provider = ?provider_id, model = ?model_override))]
    pub async fn process_message_with_agent_config(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
        continuity_key: Option<&str>,
        user_id: Option<&str>,
        provider_id: Option<&str>,
        model_override: Option<&str>,
        system_prompt_override: Option<&str>,
        max_tokens_override: Option<u32>,
    ) -> Result<String> {
        // Resolve provider: first try explicit provider_id, then try deriving from model_override
        let provider: Arc<dyn LlmProvider> = if let Some(pid) = provider_id {
            self.get_provider(pid)
                .ok_or_else(|| Error::Agent(format!("provider '{pid}' not found")))?
        } else if let Some(model) = model_override {
            if let Some(resolved_provider_id) = resolve_provider_from_model(model) {
                if let Some(provider) = self.get_provider(&resolved_provider_id) {
                    info!(
                        "Resolved provider '{}' from model override '{}'",
                        resolved_provider_id, model
                    );
                    provider
                } else {
                    // Provider not registered — if model uses `org/model` format, try openrouter
                    // (it proxies minimax, yi, moonshot, etc.) before falling to the global default.
                    if model.contains('/') {
                        if let Some(or_provider) = self.get_provider("openrouter") {
                            warn!(
                                "Provider '{}' not registered; routing '{}' via openrouter",
                                resolved_provider_id, model
                            );
                            or_provider
                        } else {
                            warn!(
                                "Provider '{}' not found, falling back to default",
                                resolved_provider_id
                            );
                            self.default_provider()
                                .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?
                        }
                    } else {
                        warn!(
                            "Provider '{}' not found, falling back to default",
                            resolved_provider_id
                        );
                        self.default_provider()
                            .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?
                    }
                }
            } else {
                // No provider prefix in model, use default
                self.default_provider()
                    .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?
            }
        } else {
            self.default_provider()
                .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?
        };

        // Plan 0250 (GAR-771): resolve override → config prompt → default
        // persona. An explicit prompt always wins; the persona only fills in
        // when nothing is configured (and not in Neutral mode).
        let explicit_prompt = system_prompt_override
            .map(|s| s.to_string())
            .or_else(|| self.system_prompt.clone());
        let effective_system_prompt = self.base_system_prompt(explicit_prompt.as_deref());
        let effective_model = model_override
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(|m| m.to_string())
            .unwrap_or_default();
        let effective_max_tokens = max_tokens_override.or(self.max_tokens).unwrap_or(4096);

        let memory_context = match self
            .recall_context(user_text, Some(session_id), continuity_key, 5)
            .await
        {
            Ok(entries) if !entries.is_empty() => {
                let context: Vec<String> = entries.iter().map(|e| e.content.clone()).collect();
                Some(format!(
                    "Relevant context from memory:\n- {}",
                    context.join("\n- ")
                ))
            }
            Err(e) => {
                warn!("memory recall failed, continuing without context: {}", e);
                None
            }
            _ => None,
        };

        let system = match (&effective_system_prompt, memory_context) {
            (Some(prompt), Some(ctx)) => Some(format!("{prompt}\n\n{ctx}")),
            (Some(prompt), None) => Some(prompt.clone()),
            (None, Some(ctx)) => Some(ctx),
            (None, None) => None,
        };

        let tool_defs = self.tool_definitions();
        let (provider, effective_model) =
            self.apply_tools_model_override(provider, effective_model, tool_defs.len());
        info!(
            "agent starting: provider={}, tools={}, history_msgs={}",
            provider.provider_id(),
            tool_defs.len(),
            conversation_history.len()
        );

        // GAR-187: detect if the user approved a pending tool confirmation
        let is_confirmation_approved =
            detect_confirmation_approval(conversation_history, user_text);

        // GAR-208: apply sliding window before building the message list
        let windowed = self.context_policy.apply_window(conversation_history);
        let mut messages: Vec<ChatMessage> = windowed.to_vec();
        messages.push(ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text(user_text.to_string()),
        });

        let max_ctx = self.max_context_tokens.unwrap_or(100_000);
        trim_messages_to_budget(&mut messages, &system, &tool_defs, max_ctx);

        let mut budget = match self.max_tool_calls {
            Some(limit) => ExecutionBudget::com_limite(limit),
            None => ExecutionBudget::padrao(),
        };

        // Reset turn counter at the start of processing a new user message
        budget.resetar_turno();

        loop {
            // Auto-reset turn limit when reached (but task limit not reached)
            // This allows multi-turn agent loops without failing
            if budget.atingiu_limite_turno() {
                budget.resetar_turno();
                info!("auto-reset turn budget, continuing agent loop");
            }

            // Check if task limit is reached (hard limit)
            if !budget.pode_chamar_ferramenta() {
                return Err(Error::Agent(format!(
                    "execution budget exceeded: {}",
                    budget.status()
                )));
            }

            let request = LlmRequest {
                model: effective_model.clone(),
                messages: messages.clone(),
                system: system.clone(),
                max_tokens: Some(effective_max_tokens),
                temperature: None,
                tools: tool_defs.clone(),
            };

            let response = self.complete_with_fallback(&provider, &request).await?;

            let has_tool_use = response
                .content
                .iter()
                .any(|block| matches!(block, ContentBlock::ToolUse { .. }));

            if !has_tool_use {
                let final_text = extract_text(&response.content);
                info!(
                    "agent finished without tool calls (stop_reason={:?}, response_len={})",
                    response.stop_reason,
                    final_text.len()
                );
                if let Err(e) = self
                    .remember_turn(session_id, continuity_key, user_id, user_text, &final_text)
                    .await
                {
                    warn!("failed to store turn in memory: {}", e);
                }
                return Ok(final_text);
            }

            // Agent chose to call tools — log which ones
            let tool_names: Vec<&str> = response
                .content
                .iter()
                .filter_map(|b| {
                    if let ContentBlock::ToolUse { name, .. } = b {
                        Some(name.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            info!("agent calling tools: {:?}", tool_names);

            messages.push(ChatMessage {
                role: ChatRole::Assistant,
                content: MessagePart::Parts(response.content.clone()),
            });

            let mut tool_results = Vec::new();
            for block in &response.content {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    let context = crate::tools::ToolContext {
                        session_id: session_id.to_string(),
                        user_id: user_id.map(|s| s.to_string()),
                        is_heartbeat: false,
                        is_confirmation_approved,
                        working_dir: None,
                        project_id: None,
                    };

                    // registra chamada com payload para detecção de loop por assinatura
                    budget.registrar_chamada(name, input);

                    // detecta loop
                    if budget.detectar_loop_ferramenta() {
                        return Err(Error::Agent(format!("tool loop detected: {}", name)));
                    }

                    // executa com timeout
                    let output = match self.find_tool(name) {
                        Some(tool) => {
                            match timeout(budget.timeout(), tool.execute(&context, input.clone()))
                                .await
                            {
                                Ok(result) => {
                                    result.unwrap_or_else(|e| ToolOutput::error(e.to_string()))
                                }
                                Err(_) => ToolOutput::error(format!("tool timeout: {}", name)),
                            }
                        }
                        None => ToolOutput::error(format!("unknown tool: {}", name)),
                    };
                    info!("tool '{}' result: is_error={}", name, output.is_error);

                    // GAR-187: pause agent loop if tool requires user confirmation
                    if output.requires_confirmation {
                        tracing::info!(session = %session_id, "agent paused: awaiting user confirmation");
                        tool_results.push(ContentBlock::ToolResult {
                            tool_use_id: id.clone(),
                            content: output.content.clone(),
                        });
                        messages.push(ChatMessage {
                            role: ChatRole::User,
                            content: MessagePart::Parts(tool_results),
                        });
                        return Ok(output.content);
                    }

                    tool_results.push(ContentBlock::ToolResult {
                        tool_use_id: id.clone(),
                        content: output.content,
                    });
                }
            }

            messages.push(ChatMessage {
                role: ChatRole::User,
                content: MessagePart::Parts(tool_results),
            });
        }
    }

    #[instrument(
        skip_all,
        fields(
            session_id = %session_id,
            has_continuity = continuity_key.is_some(),
            has_user_id = user_id.is_some(),
            is_heartbeat,
            provider_id = tracing::field::Empty,
        )
    )]
    async fn process_message_impl(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
        continuity_key: Option<&str>,
        user_id: Option<&str>,
        is_heartbeat: bool,
    ) -> Result<String> {
        let provider: Arc<dyn LlmProvider> = self
            .default_provider()
            .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?;
        tracing::Span::current().record("provider_id", provider.provider_id());

        // Build system message: system_prompt + memory context
        let memory_context = match self
            .recall_context(user_text, Some(session_id), continuity_key, 5)
            .await
        {
            Ok(entries) if !entries.is_empty() => {
                let context: Vec<String> = entries.iter().map(|e| e.content.clone()).collect();
                Some(format!(
                    "Relevant context from memory:\n- {}",
                    context.join("\n- ")
                ))
            }
            Err(e) => {
                warn!("memory recall failed, continuing without context: {}", e);
                None
            }
            _ => None,
        };

        // Plan 0250 (GAR-771): apply default persona fallback here too.
        let effective_system_prompt = self.base_system_prompt(self.system_prompt.as_deref());
        let system = match (&effective_system_prompt, memory_context) {
            (Some(prompt), Some(ctx)) => Some(format!("{prompt}\n\n{ctx}")),
            (Some(prompt), None) => Some(prompt.clone()),
            (None, Some(ctx)) => Some(ctx),
            (None, None) => None,
        };

        let tool_defs = self.tool_definitions();
        let (provider, tools_model_override) =
            self.apply_tools_model_override(provider, String::new(), tool_defs.len());

        // GAR-208: apply sliding window before building the message list
        let windowed = self.context_policy.apply_window(conversation_history);
        let mut messages: Vec<ChatMessage> = windowed.to_vec();
        messages.push(ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text(user_text.to_string()),
        });

        // Trim conversation history to fit context window
        let max_ctx = self.max_context_tokens.unwrap_or(100_000);
        trim_messages_to_budget(&mut messages, &system, &tool_defs, max_ctx);

        // GAR-187: detect if the user approved a pending tool confirmation
        let is_confirmation_approved =
            detect_confirmation_approval(conversation_history, user_text);

        let mut budget = match self.max_tool_calls {
            Some(limit) => ExecutionBudget::com_limite(limit),
            None => ExecutionBudget::padrao(),
        };

        // Reset turn counter at the start of processing a new user message
        budget.resetar_turno();

        loop {
            // Check if turn or task limit reached
            if budget.atingiu_limite_turno() {
                return Err(Error::Agent(format!(
                    "turn budget exceeded: {}",
                    budget.status()
                )));
            }

            // Check if task limit is reached (hard limit)
            if !budget.pode_chamar_ferramenta() {
                return Err(Error::Agent(format!(
                    "execution budget exceeded: {}",
                    budget.status()
                )));
            }

            let request = LlmRequest {
                model: tools_model_override.clone(),
                messages: messages.clone(),
                system: system.clone(),
                max_tokens: Some(self.max_tokens.unwrap_or(4096)),
                temperature: None,
                tools: tool_defs.clone(),
            };

            let response = provider.complete(&request).await?;

            let has_tool_use = response
                .content
                .iter()
                .any(|block| matches!(block, ContentBlock::ToolUse { .. }));

            if !has_tool_use {
                let final_text = extract_text(&response.content);

                // Store turn in memory (best-effort)
                if let Err(e) = self
                    .remember_turn(session_id, continuity_key, user_id, user_text, &final_text)
                    .await
                {
                    warn!("failed to store turn in memory: {}", e);
                }

                // Auto-learning: extrair fatos da mensagem do usuário
                let facts_result = self.memory_extractor.extract_facts(self, user_text).await;
                if let Ok(facts) = facts_result {
                    for fact in facts {
                        // Validar que o fato tem valores não vazios
                        if fact.confidence >= 0.80
                            && !fact.key.trim().is_empty()
                            && !fact.value.trim().is_empty()
                        {
                            let content = format!(
                                "[FACT] type={} key={} value={} confidence={:.2}",
                                fact.fact_type, fact.key, fact.value, fact.confidence
                            );
                            if let Some(memory) = &self.memory {
                                // Store fact in memory with embedding
                                let embedding = self.embed_document(&content).await;
                                let fact_embedding_model = self.embedding_model_for(&embedding);
                                let _ = memory
                                    .remember(NewMemoryEntry {
                                        tenant_id: "default".to_string(),
                                        session_id: session_id.to_string(),
                                        channel_id: None,
                                        user_id: user_id.map(|s| s.to_string()),
                                        continuity_key: continuity_key.map(|s| s.to_string()),
                                        role: MemoryRole::User,
                                        content,
                                        embedding,
                                        embedding_model: fact_embedding_model,
                                        metadata: serde_json::json!({ "kind": "learned_fact" }),
                                    })
                                    .await;
                                info!("stored learned fact: {}={}", fact.key, fact.value);
                            }
                        }
                    }
                }

                budget.resetar_turno();
                return Ok(final_text);
            }

            // Append the assistant's response (including tool_use blocks) to history
            messages.push(ChatMessage {
                role: ChatRole::Assistant,
                content: MessagePart::Parts(response.content.clone()),
            });

            // Execute each tool and collect results
            let mut tool_results = Vec::new();
            let mut confirmation_response: Option<String> = None;
            for block in &response.content {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    let context = ToolContext {
                        session_id: session_id.to_string(),
                        user_id: user_id.map(|s| s.to_string()),
                        is_heartbeat,
                        is_confirmation_approved,
                        working_dir: None,
                        project_id: None,
                    };

                    // registra chamada com payload para detecção de loop por assinatura
                    budget.registrar_chamada(name, input);

                    // detecta loop
                    if budget.detectar_loop_ferramenta() {
                        return Err(Error::Agent(format!("tool loop detected: {}", name)));
                    }

                    // executa com timeout
                    let output = match self.find_tool(name) {
                        Some(tool) => {
                            match timeout(budget.timeout(), tool.execute(&context, input.clone()))
                                .await
                            {
                                Ok(result) => {
                                    result.unwrap_or_else(|e| ToolOutput::error(e.to_string()))
                                }
                                Err(_) => ToolOutput::error(format!("tool timeout: {}", name)),
                            }
                        }
                        None => ToolOutput::error(format!("unknown tool: {}", name)),
                    };

                    // GAR-187: pause agent loop if tool requires user confirmation
                    if output.requires_confirmation {
                        tracing::info!(session = %session_id, "agent paused: awaiting user confirmation");
                        tool_results.push(ContentBlock::ToolResult {
                            tool_use_id: id.clone(),
                            content: output.content.clone(),
                        });
                        confirmation_response = Some(output.content);
                        break;
                    }

                    tool_results.push(ContentBlock::ToolResult {
                        tool_use_id: id.clone(),
                        content: output.content,
                    });
                }
            }

            // Append tool results as a user message
            messages.push(ChatMessage {
                role: ChatRole::User,
                content: MessagePart::Parts(tool_results),
            });

            // GAR-187: if a confirmation was requested, return the prompt immediately
            if let Some(confirmation_msg) = confirmation_response {
                return Ok(confirmation_msg);
            }
        }
    }

    /// Run the conversation loop with streaming. Text deltas are sent through
    /// `delta_tx` as they arrive. Returns the final accumulated response text.
    pub async fn process_message_streaming(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
        delta_tx: mpsc::Sender<String>,
        model_override: Option<&str>,
    ) -> Result<String> {
        self.process_message_streaming_with_context(
            session_id,
            user_text,
            conversation_history,
            delta_tx,
            None,
            None,
            model_override,
        )
        .await
    }

    /// Streaming variant with continuity/user context for shared memory.
    #[instrument(
        skip_all,
        fields(
            session_id = %session_id,
            has_continuity = continuity_key.is_some(),
            has_user_id = user_id.is_some(),
        )
    )]
    pub async fn process_message_streaming_with_context(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
        delta_tx: mpsc::Sender<String>,
        continuity_key: Option<&str>,
        user_id: Option<&str>,
        model_override: Option<&str>,
    ) -> Result<String> {
        self.process_message_streaming_with_agent_config(
            session_id,
            user_text,
            conversation_history,
            delta_tx,
            continuity_key,
            user_id,
            None,
            model_override,
            None,
            None,
        )
        .await
    }

    /// Streaming variant with explicit agent config overrides (for multi-agent routing or dynamic models).
    #[allow(clippy::too_many_arguments)]
    #[instrument(
        skip_all,
        fields(
            session_id = %session_id,
            has_continuity = continuity_key.is_some(),
            has_user_id = user_id.is_some(),
            provider_id = tracing::field::Empty,
            model = tracing::field::Empty,
        )
    )]
    #[allow(clippy::too_many_arguments)]
    pub async fn process_message_streaming_with_agent_config(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
        delta_tx: mpsc::Sender<String>,
        continuity_key: Option<&str>,
        user_id: Option<&str>,
        provider_id: Option<&str>,
        model_override: Option<&str>,
        system_prompt_override: Option<&str>,
        max_tokens_override: Option<u32>,
    ) -> Result<String> {
        self.stream_turn_with_sink(
            session_id,
            user_text,
            conversation_history,
            TurnSink::Text(delta_tx),
            continuity_key,
            user_id,
            provider_id,
            model_override,
            system_prompt_override,
            max_tokens_override,
        )
        .await
    }

    /// Como [`Self::process_message_streaming`], mas entregando o **fluxo
    /// completo** do turno: texto e ciclo de vida das ferramentas (#937).
    ///
    /// Existe para o `garra chat` poder desenhar o que o agente esta fazendo
    /// sem ler log. Os outros chamadores continuam no caminho de texto e nao
    /// pagam nada por isto — ver [`TurnSink`].
    #[allow(clippy::too_many_arguments)]
    pub async fn process_message_streaming_with_events(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
        events_tx: mpsc::Sender<crate::turn_events::TurnEvent>,
        continuity_key: Option<&str>,
        user_id: Option<&str>,
        provider_id: Option<&str>,
        model_override: Option<&str>,
        system_prompt_override: Option<&str>,
        max_tokens_override: Option<u32>,
    ) -> Result<String> {
        self.stream_turn_with_sink(
            session_id,
            user_text,
            conversation_history,
            TurnSink::Events(events_tx),
            continuity_key,
            user_id,
            provider_id,
            model_override,
            system_prompt_override,
            max_tokens_override,
        )
        .await
    }

    /// O turno em si. Um sink so, entao a ordem entre texto e evento de
    /// ferramenta e a ordem de emissao (ver `turn_events`).
    #[allow(clippy::too_many_arguments)]
    async fn stream_turn_with_sink(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
        sink: TurnSink,
        continuity_key: Option<&str>,
        user_id: Option<&str>,
        provider_id: Option<&str>,
        model_override: Option<&str>,
        system_prompt_override: Option<&str>,
        max_tokens_override: Option<u32>,
    ) -> Result<String> {
        // Resolve provider: first try explicit provider_id, then try deriving from model_override
        let provider: Arc<dyn LlmProvider> = if let Some(pid) = provider_id {
            self.get_provider(pid)
                .ok_or_else(|| Error::Agent(format!("provider '{pid}' not found")))?
        } else if let Some(model) = model_override {
            if let Some(resolved_provider_id) = resolve_provider_from_model(model) {
                if let Some(provider) = self.get_provider(&resolved_provider_id) {
                    info!(
                        "Resolved provider '{}' from model override '{}'",
                        resolved_provider_id, model
                    );
                    provider
                } else {
                    // Provider not registered — if model uses `org/model` format, try openrouter
                    // (it proxies minimax, yi, moonshot, etc.) before falling to the global default.
                    if model.contains('/') {
                        if let Some(or_provider) = self.get_provider("openrouter") {
                            warn!(
                                "Provider '{}' not registered; routing '{}' via openrouter",
                                resolved_provider_id, model
                            );
                            or_provider
                        } else {
                            warn!(
                                "Provider '{}' not found, falling back to default",
                                resolved_provider_id
                            );
                            self.default_provider()
                                .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?
                        }
                    } else {
                        warn!(
                            "Provider '{}' not found, falling back to default",
                            resolved_provider_id
                        );
                        self.default_provider()
                            .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?
                    }
                }
            } else {
                // No provider prefix in model, use default
                self.default_provider()
                    .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?
            }
        } else {
            self.default_provider()
                .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?
        };

        // Plan 0250 (GAR-771): resolve override → config prompt → default
        // persona. An explicit prompt always wins; the persona only fills in
        // when nothing is configured (and not in Neutral mode).
        let explicit_prompt = system_prompt_override
            .map(|s| s.to_string())
            .or_else(|| self.system_prompt.clone());
        let effective_system_prompt = self.base_system_prompt(explicit_prompt.as_deref());
        let effective_model = model_override
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(|m| m.to_string())
            .unwrap_or_default();
        let effective_max_tokens = max_tokens_override.or(self.max_tokens).unwrap_or(4096);

        // Build system message (same as process_message)
        let memory_context = match self
            .recall_context(user_text, Some(session_id), continuity_key, 5)
            .await
        {
            Ok(entries) if !entries.is_empty() => {
                let context: Vec<String> = entries.iter().map(|e| e.content.clone()).collect();
                Some(format!(
                    "Relevant context from memory:\n- {}",
                    context.join("\n- ")
                ))
            }
            Err(e) => {
                warn!("memory recall failed, continuing without context: {}", e);
                None
            }
            _ => None,
        };

        let system = match (&effective_system_prompt, memory_context) {
            (Some(prompt), Some(ctx)) => Some(format!("{prompt}\n\n{ctx}")),
            (Some(prompt), None) => Some(prompt.clone()),
            (None, Some(ctx)) => Some(ctx),
            (None, None) => None,
        };

        let tool_defs = self.tool_definitions();
        let (provider, effective_model) =
            self.apply_tools_model_override(provider, effective_model, tool_defs.len());
        info!(
            "agent streaming: provider={}, tools={}, history_msgs={}",
            provider.provider_id(),
            tool_defs.len(),
            conversation_history.len()
        );

        // GAR-187: detect if the user approved a pending tool confirmation
        let is_confirmation_approved =
            detect_confirmation_approval(conversation_history, user_text);

        // GAR-208: apply sliding window before building the message list
        let windowed = self.context_policy.apply_window(conversation_history);
        let mut messages: Vec<ChatMessage> = windowed.to_vec();
        messages.push(ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text(user_text.to_string()),
        });

        let max_ctx = self.max_context_tokens.unwrap_or(100_000);
        trim_messages_to_budget(&mut messages, &system, &tool_defs, max_ctx);

        let mut full_response = String::new();

        let mut budget = match self.max_tool_calls {
            Some(limit) => ExecutionBudget::com_limite(limit),
            None => ExecutionBudget::padrao(),
        };

        // Reset turn counter at the start of processing a new user message
        budget.resetar_turno();

        loop {
            // Auto-reset turn limit when reached (but task limit not reached)
            // This allows multi-turn agent loops without failing
            if budget.atingiu_limite_turno() {
                budget.resetar_turno();
                info!("auto-reset turn budget, continuing agent loop");
            }

            // Check if task limit is reached (hard limit)
            if !budget.pode_chamar_ferramenta() {
                return Err(Error::Agent(format!(
                    "execution budget exceeded: {}",
                    budget.status()
                )));
            }
            let request = LlmRequest {
                model: effective_model.clone(),
                messages: messages.clone(),
                system: system.clone(),
                max_tokens: Some(effective_max_tokens),
                temperature: None,
                tools: tool_defs.clone(),
            };

            tracing::info!(
                "Sending LlmRequest to provider={}, model={}, tools_count={}",
                provider.provider_id(),
                request.model,
                request.tools.len()
            );

            // Try streaming with fallback; fall back to non-streaming if unsupported
            let stream_result = self
                .stream_complete_with_fallback(&provider, &request)
                .await;

            match stream_result {
                Ok(mut stream) => {
                    // Consume stream, collecting the full response and forwarding text deltas
                    let mut response_text = String::new();
                    let mut tool_uses: Vec<(String, String, String)> = Vec::new(); // (id, name, input_json)
                    let mut current_tool: Option<(String, String, String)> = None;
                    let mut _stop_reason: Option<String> = None;
                    let mut debug_event_count = 0;

                    while let Some(event) = stream.next().await {
                        match event? {
                            StreamEvent::TextDelta(text) => {
                                response_text.push_str(&text);
                                sink.text(text).await;
                            }
                            StreamEvent::ToolUseStart { id, name, .. } => {
                                current_tool = Some((id, name, String::new()));
                            }
                            StreamEvent::InputJsonDelta(json) => {
                                if let Some((_, _, ref mut input)) = current_tool {
                                    input.push_str(&json);
                                }
                            }
                            StreamEvent::ContentBlockStop { .. } => {
                                if let Some(tool) = current_tool.take() {
                                    tool_uses.push(tool);
                                }
                            }
                            StreamEvent::MessageDelta {
                                stop_reason: sr, ..
                            } => {
                                _stop_reason = sr;
                            }
                            StreamEvent::MessageStop => break,
                        }
                        debug_event_count += 1;
                    }

                    // Some OpenAI-compatible streaming APIs (e.g. OpenRouter via /v1/chat/completions)
                    // don't emit an explicit ContentBlockStop event for tool calls.
                    // If we ended the stream and still have a pending tool, flush it so it executes.
                    if let Some(tool) = current_tool.take() {
                        tool_uses.push(tool);
                    }

                    tracing::info!(
                        "Stream finished. events={}, text_len={}, tool_uses={}",
                        debug_event_count,
                        response_text.len(),
                        tool_uses.len()
                    );

                    if tool_uses.is_empty() {
                        full_response.push_str(&response_text);

                        if let Err(e) = self
                            .remember_turn(
                                session_id,
                                continuity_key,
                                user_id,
                                user_text,
                                &full_response,
                            )
                            .await
                        {
                            warn!("failed to store turn in memory: {}", e);
                        }

                        return Ok(full_response);
                    }

                    // Build assistant response with text + tool_use blocks
                    let mut content_blocks = Vec::new();
                    if !response_text.is_empty() {
                        content_blocks.push(ContentBlock::Text {
                            text: response_text.clone(),
                        });
                        full_response.push_str(&response_text);
                    }

                    for (id, name, input_json) in &tool_uses {
                        let input: serde_json::Value =
                            serde_json::from_str(input_json).unwrap_or_default();
                        content_blocks.push(ContentBlock::ToolUse {
                            id: id.clone(),
                            name: name.clone(),
                            input,
                        });
                    }

                    messages.push(ChatMessage {
                        role: ChatRole::Assistant,
                        content: MessagePart::Parts(content_blocks),
                    });

                    // Execute tools
                    let mut tool_results = Vec::new();
                    let mut confirmation_response: Option<String> = None;
                    for (id, name, input_json) in &tool_uses {
                        let input: serde_json::Value =
                            serde_json::from_str(input_json).unwrap_or_default();
                        let context = ToolContext {
                            session_id: session_id.to_string(),
                            user_id: user_id.map(|s| s.to_string()),
                            is_heartbeat: false,
                            is_confirmation_approved,
                            working_dir: None,
                            project_id: None,
                        };

                        // registra chamada com payload para detecção de loop por assinatura
                        budget.registrar_chamada(name, &input);

                        // detecta loop
                        if budget.detectar_loop_ferramenta() {
                            return Err(Error::Agent(format!("tool loop detected: {}", name)));
                        }

                        // #937: este e o caminho de streaming nativo; o de
                        // fallback tem a instrumentacao equivalente logo
                        // adiante. Os dois precisam dela — o Ollama, provedor
                        // padrao do projeto, nao implementa `stream_complete`
                        // e cai justamente no outro.
                        if sink.wants_tool_events() {
                            sink.tool_started(name, summarize_tool_input(name, &input))
                                .await;
                        }
                        let iniciado_em = std::time::Instant::now();

                        // executa com timeout
                        let output = match self.find_tool(name) {
                            Some(tool) => {
                                match timeout(budget.timeout(), tool.execute(&context, input)).await
                                {
                                    Ok(result) => {
                                        result.unwrap_or_else(|e| ToolOutput::error(e.to_string()))
                                    }
                                    Err(_) => ToolOutput::error(format!("tool timeout: {}", name)),
                                }
                            }
                            None => ToolOutput::error(format!("unknown tool: {}", name)),
                        };

                        if sink.wants_tool_events() {
                            let ok = !output.is_error;
                            sink.tool_finished(
                                name,
                                iniciado_em.elapsed(),
                                ok,
                                summarize_tool_output(&output.content, ok),
                            )
                            .await;
                        }

                        // GAR-187: pause agent loop if tool requires user confirmation
                        if output.requires_confirmation {
                            tracing::info!(session = %session_id, "agent paused (streaming): awaiting user confirmation");
                            tool_results.push(ContentBlock::ToolResult {
                                tool_use_id: id.clone(),
                                content: output.content.clone(),
                            });
                            confirmation_response = Some(output.content);
                            break;
                        }

                        tool_results.push(ContentBlock::ToolResult {
                            tool_use_id: id.clone(),
                            content: output.content,
                        });
                    }

                    messages.push(ChatMessage {
                        role: ChatRole::User,
                        content: MessagePart::Parts(tool_results),
                    });

                    // GAR-187: if confirmation needed, send prompt via stream and return
                    if let Some(confirmation_msg) = confirmation_response {
                        sink.text(confirmation_msg.clone()).await;
                        full_response.push_str(&confirmation_msg);
                        return Ok(full_response);
                    }

                    // Add separator between iterations
                    if !full_response.is_empty() {
                        full_response.push_str("\n\n");
                        sink.text("\n\n".to_string()).await;
                    }
                }
                Err(_) => {
                    // Streaming not supported — fall back to non-streaming with retry/fallback
                    let response = self.complete_with_fallback(&provider, &request).await?;

                    let tool_calls_count = response
                        .content
                        .iter()
                        .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
                        .count();

                    tracing::info!(
                        "Batch fallback finished. text_len={}, tool_uses={}",
                        extract_text(&response.content).len(),
                        tool_calls_count
                    );

                    let has_tool_use = tool_calls_count > 0;

                    if !has_tool_use {
                        let final_text = extract_text(&response.content);
                        sink.text(final_text.clone()).await;
                        full_response.push_str(&final_text);

                        if let Err(e) = self
                            .remember_turn(
                                session_id,
                                continuity_key,
                                user_id,
                                user_text,
                                &full_response,
                            )
                            .await
                        {
                            warn!("failed to store turn in memory: {}", e);
                        }

                        return Ok(full_response);
                    }

                    messages.push(ChatMessage {
                        role: ChatRole::Assistant,
                        content: MessagePart::Parts(response.content.clone()),
                    });

                    let mut tool_results = Vec::new();
                    let mut confirmation_response: Option<String> = None;
                    for block in &response.content {
                        if let ContentBlock::ToolUse { id, name, input } = block {
                            let context = ToolContext {
                                session_id: session_id.to_string(),
                                user_id: user_id.map(|s| s.to_string()),
                                is_heartbeat: false,
                                is_confirmation_approved,
                                working_dir: None,
                                project_id: None,
                            };

                            // registra chamada com payload para detecção de loop por assinatura
                            budget.registrar_chamada(name, input);

                            // detecta loop
                            if budget.detectar_loop_ferramenta() {
                                return Err(Error::Agent(format!("tool loop detected: {}", name)));
                            }

                            // #937: o resumo do input so e montado quando alguem
                            // vai desenha-lo. `summarize_tool_input` ja redige
                            // segredo na origem.
                            if sink.wants_tool_events() {
                                sink.tool_started(name, summarize_tool_input(name, input))
                                    .await;
                            }
                            let iniciado_em = std::time::Instant::now();

                            // executa com timeout
                            let output = match self.find_tool(name) {
                                Some(tool) => {
                                    match timeout(
                                        budget.timeout(),
                                        tool.execute(&context, input.clone()),
                                    )
                                    .await
                                    {
                                        Ok(result) => result
                                            .unwrap_or_else(|e| ToolOutput::error(e.to_string())),
                                        Err(_) => {
                                            ToolOutput::error(format!("tool timeout: {}", name))
                                        }
                                    }
                                }
                                None => ToolOutput::error(format!("unknown tool: {}", name)),
                            };

                            if sink.wants_tool_events() {
                                let ok = !output.is_error;
                                sink.tool_finished(
                                    name,
                                    iniciado_em.elapsed(),
                                    ok,
                                    summarize_tool_output(&output.content, ok),
                                )
                                .await;
                            }

                            // GAR-187: pause if tool requires user confirmation
                            if output.requires_confirmation {
                                tracing::info!(session = %session_id, "agent paused (streaming fallback): awaiting user confirmation");
                                tool_results.push(ContentBlock::ToolResult {
                                    tool_use_id: id.clone(),
                                    content: output.content.clone(),
                                });
                                confirmation_response = Some(output.content);
                                break;
                            }

                            tool_results.push(ContentBlock::ToolResult {
                                tool_use_id: id.clone(),
                                content: output.content,
                            });
                        }
                    }

                    messages.push(ChatMessage {
                        role: ChatRole::User,
                        content: MessagePart::Parts(tool_results),
                    });

                    // GAR-187: if confirmation needed, send prompt via stream and return
                    if let Some(confirmation_msg) = confirmation_response {
                        sink.text(confirmation_msg.clone()).await;
                        full_response.push_str(&confirmation_msg);
                        return Ok(full_response);
                    }
                }
            }
        }
    }

    // ── GAR-210: Retry + fallback helpers ────────────────────────────────────

    /// Try `primary` provider with exponential-backoff retries, then fall through
    /// the configured `fallback_providers_list` on retryable errors (429, 5xx).
    ///
    /// `pub` so the Anthropic-compatible shim (`POST /v1/messages`) can reuse
    /// the primary→backup chain instead of reimplementing it. That endpoint is a
    /// proxy, not an agent: it must not go through `process_message_*`, which
    /// injects GarraIA's own tools and executes them itself — the caller needs
    /// the raw `tool_use` blocks back so it can run its own tools.
    pub async fn complete_with_fallback(
        &self,
        primary: &Arc<dyn LlmProvider>,
        request: &LlmRequest,
    ) -> Result<LlmResponse> {
        let primary_id = primary.provider_id().to_string();
        let retry_policy = &self.resilience.retry_policy;

        // --- Try primary with retries ---
        let primary_cb = self.resilience.circuit_breaker(&primary_id).await;
        if primary_cb.allow_request().await {
            let mut last_err: Option<Error> = None;
            for attempt in 0..=retry_policy.max_retries {
                match primary.complete(request).await {
                    Ok(resp) => {
                        primary_cb.record_success().await;
                        return Ok(resp);
                    }
                    Err(e) if is_retryable_error(&e) => {
                        warn!(
                            "provider '{}' attempt {} failed (retryable): {}",
                            primary_id,
                            attempt + 1,
                            e
                        );
                        primary_cb.record_failure().await;
                        if attempt < retry_policy.max_retries {
                            tokio::time::sleep(retry_policy.delay_for_attempt(attempt)).await;
                        }
                        last_err = Some(e);
                    }
                    Err(e) => return Err(e),
                }
            }
            if let Some(e) = last_err {
                warn!("provider '{}' exhausted retries: {}", primary_id, e);
            }
        } else {
            warn!("provider '{}' circuit open, skipping primary", primary_id);
        }

        // --- Try fallback providers ---
        let fallbacks = self.fallback_providers_list.read().unwrap().clone();
        for fallback_id in &fallbacks {
            if *fallback_id == primary_id {
                continue;
            }
            let Some(fallback) = self.get_provider(fallback_id) else {
                continue;
            };
            let cb = self.resilience.circuit_breaker(fallback_id).await;
            if !cb.allow_request().await {
                warn!("fallback '{}' circuit open, skipping", fallback_id);
                continue;
            }
            info!("provider fallback: trying '{}'", fallback_id);
            match fallback.complete(request).await {
                Ok(resp) => {
                    cb.record_success().await;
                    return Ok(resp);
                }
                Err(e) => {
                    warn!("fallback '{}' failed: {}", fallback_id, e);
                    cb.record_failure().await;
                }
            }
        }

        Err(Error::Agent(format!(
            "all providers failed (primary: {primary_id}, fallbacks: [{}])",
            fallbacks.join(", ")
        )))
    }

    /// Like `complete_with_fallback` but for streaming.
    /// Tries primary, then fallbacks, returning the first successful stream.
    /// Streaming counterpart of [`complete_with_fallback`], `pub` for the same
    /// reason.
    pub async fn stream_complete_with_fallback(
        &self,
        primary: &Arc<dyn LlmProvider>,
        request: &LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let primary_id = primary.provider_id().to_string();

        match primary.stream_complete(request).await {
            Ok(stream) => return Ok(stream),
            Err(e) if is_retryable_error(&e) => {
                warn!(
                    "streaming provider '{}' failed, trying fallbacks: {}",
                    primary_id, e
                );
                let cb = self.resilience.circuit_breaker(&primary_id).await;
                cb.record_failure().await;
            }
            Err(e) => return Err(e),
        }

        let fallbacks = self.fallback_providers_list.read().unwrap().clone();
        for fallback_id in &fallbacks {
            if *fallback_id == primary_id {
                continue;
            }
            let Some(fallback) = self.get_provider(fallback_id) else {
                continue;
            };
            let cb = self.resilience.circuit_breaker(fallback_id).await;
            if !cb.allow_request().await {
                continue;
            }
            info!("streaming fallback: trying '{}'", fallback_id);
            match fallback.stream_complete(request).await {
                Ok(stream) => return Ok(stream),
                Err(e) => {
                    warn!("streaming fallback '{}' failed: {}", fallback_id, e);
                    cb.record_failure().await;
                }
            }
        }

        Err(Error::Agent(format!(
            "all streaming providers failed (primary: {primary_id})"
        )))
    }

    pub async fn health_check_all(&self) -> Result<Vec<(String, bool)>> {
        let providers: Vec<Arc<dyn LlmProvider>> = self.providers.read().unwrap().clone();
        let checks = providers.iter().map(|provider| async {
            let provider_id = provider.provider_id().to_string();
            let ok = provider.health_check().await.unwrap_or(false);
            (provider_id, ok)
        });

        Ok(join_all(checks).await)
    }

    async fn remember_system_event(
        &self,
        session_id: &str,
        continuity_key: Option<&str>,
        event: &str,
        content: &str,
    ) -> Result<()> {
        let Some(memory) = &self.memory else {
            return Ok(());
        };

        memory
            .remember(NewMemoryEntry {
                tenant_id: "default".to_string(),
                session_id: session_id.to_string(),
                channel_id: None,
                user_id: None,
                continuity_key: continuity_key.map(|s| s.to_string()),
                role: MemoryRole::System,
                content: content.to_string(),
                embedding: None,
                embedding_model: None,
                metadata: serde_json::json!({ "kind": event }),
            })
            .await?;

        Ok(())
    }

    /// Embedding de um turno, a menos que a politica de ruido recuse (#952).
    ///
    /// A entrada e gravada de qualquer jeito: o que se decide aqui e se ela
    /// entra no indice vetorial. Recusar cedo tambem poupa uma ida ao
    /// provider por "ok" — que numa conversa longa nao e pouco.
    ///
    /// Vale so para turno. Fato extraido (`[FACT] ...`) nao passa por aqui:
    /// ele ja e sinal filtrado por um LLM com limiar de confianca, e o texto
    /// que se grava e sempre longo.
    async fn embed_turn_unless_noise(&self, text: &str, papel: &str) -> Option<Vec<f32>> {
        if self.noise_policy.is_noise(text) {
            metrics::inc_ingested(metrics::IngestOutcome::Noise);
            debug!(
                papel,
                chars = text.chars().count(),
                "memoria: turno gravado sem vetor por ser ruido para a busca \
                 semantica (#952); a entrada continua no historico e no recall \
                 textual. Ajuste em `memory.ingestion`."
            );
            return None;
        }

        // #957: o desfecho e contado **aqui**, e nao dentro do
        // `embed_document`, porque so este nivel sabe distinguir os quatro
        // casos. La embaixo, "sem provider" e "provider falhou" saem os dois
        // como `None` — e sao a diferenca entre "ninguem configurou" e
        // "configurou e esta quebrado", que e exatamente o que o operador
        // precisa separar.
        if self.embeddings.is_none() {
            metrics::inc_ingested(metrics::IngestOutcome::NoProvider);
            return None;
        }

        let vetor = self.embed_document(text).await;
        metrics::inc_ingested(if vetor.is_some() {
            metrics::IngestOutcome::Embedded
        } else {
            metrics::IngestOutcome::Failed
        });
        vetor
    }

    async fn embed_document(&self, text: &str) -> Option<Vec<f32>> {
        let provider = self.embeddings.as_ref()?;
        // #957: a medicao cerca a chamada ao provider e nada mais. Incluir o
        // que vem antes ou depois faria o histograma medir o GarraIA em vez de
        // medir o provider, que e a pergunta que o operador tem.
        let inicio = std::time::Instant::now();
        match provider.embed_documents(&[text.to_string()]).await {
            Ok(mut vectors) => {
                metrics::record_embed_latency(
                    provider.provider_id(),
                    metrics::EmbedOp::Document,
                    inicio.elapsed().as_secs_f64(),
                );
                vectors.pop()
            }
            Err(e) => {
                // O `.ok()` que existia aqui era o primeiro elo da cadeia de
                // perda silenciosa do #948: a entrada era gravada sem vetor,
                // ficava invisivel para a busca semantica para sempre, e nada
                // no log dizia que tinha acontecido.
                //
                // O #948 tirou o silencio do log; o #957 tira do painel. Log
                // conta o caso, metrica conta a tendencia — e e a tendencia
                // que faz alguem descobrir que o provider caiu antes de o
                // recall degradar.
                metrics::inc_embed_failure(provider.provider_id(), metrics::EmbedOp::Document);
                warn!(
                    provider = provider.provider_id(),
                    model = provider.model(),
                    "memoria: embedding do documento falhou; a entrada vai ser gravada \
                     sem vetor e so volta a ser encontravel por busca semantica depois \
                     de uma reindexacao (#948): {e}"
                );
                None
            }
        }
    }

    async fn embed_query(&self, text: &str) -> Option<Vec<f32>> {
        let provider = self.embeddings.as_ref()?;
        let inicio = std::time::Instant::now();
        match provider.embed_query(text).await {
            Ok(vector) => {
                metrics::record_embed_latency(
                    provider.provider_id(),
                    metrics::EmbedOp::Query,
                    inicio.elapsed().as_secs_f64(),
                );
                Some(vector)
            }
            Err(e) => {
                metrics::inc_embed_failure(provider.provider_id(), metrics::EmbedOp::Query);
                warn!(
                    provider = provider.provider_id(),
                    model = provider.model(),
                    "memoria: embedding da consulta falhou; o recall deste turno cai \
                     para o caminho textual, sem semantica (#948): {e}"
                );
                None
            }
        }
    }

    fn embedding_model(&self) -> Option<String> {
        self.embeddings
            .as_ref()
            .map(|provider| provider.model().to_string())
    }

    /// Modelo a gravar ao lado de um embedding.
    ///
    /// `None` quando nao ha vetor: a coluna `embedding_model` descreve o
    /// vetor, entao preenche-la sem vetor faz a linha mentir — a entrada
    /// parece indexada por um modelo e nao esta. Com o filtro de modelo do
    /// #954 ativo, essa mentira ainda fazia a entrada perder o eixo semantico
    /// sem que ninguem entendesse por que.
    fn embedding_model_for(&self, embedding: &Option<Vec<f32>>) -> Option<String> {
        embedding.as_ref().and_then(|_| self.embedding_model())
    }

    /// Simple chat completion for use by memory extractor
    pub async fn chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        system: Option<String>,
        model: Option<String>,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<String> {
        let provider = self
            .default_provider()
            .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?;

        let request = LlmRequest {
            model: model.unwrap_or_default(),
            messages,
            system,
            max_tokens: Some(max_tokens.unwrap_or(4096)),
            temperature: temperature.map(|t| t as f64),
            tools: tools.unwrap_or_default(),
        };

        let response = provider.complete(&request).await?;
        Ok(extract_text(&response.content))
    }
}

impl Default for AgentRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── #957: as metricas da memoria ─────────────────────────────────────

    /// Nome + labels de tudo que foi emitido enquanto `f` rodava.
    ///
    /// O recorder e **thread-local**, nao global, de proposito: o recorder
    /// global do ecossistema `metrics` so pode ser instalado uma vez por
    /// processo, e um teste que o instalasse quebraria todos os outros que
    /// rodam em paralelo. `#[tokio::test]` usa o runtime `current_thread`,
    /// entao o guard cobre o bloco inteiro sem risco de a task migrar de
    /// thread no meio.
    async fn metricas_emitidas<F, Fut>(f: F) -> Vec<String>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let recorder = metrics_util::debugging::DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let guard = ::metrics::set_default_local_recorder(&recorder);
        f().await;
        drop(guard);

        let mut nomes: Vec<String> = snapshotter
            .snapshot()
            .into_vec()
            .into_iter()
            .map(|(chave, _, _, _)| {
                let key = chave.key();
                let mut labels: Vec<String> = key
                    .labels()
                    .map(|l| format!("{}={}", l.key(), l.value()))
                    .collect();
                labels.sort();
                if labels.is_empty() {
                    key.name().to_string()
                } else {
                    format!("{}{{{}}}", key.name(), labels.join(","))
                }
            })
            .collect();
        nomes.sort();
        nomes
    }

    /// O caso que a issue #957 descreve: falha de embedding era silenciosa. O
    /// #948 tirou o silencio do log; isto tira do painel.
    #[tokio::test]
    async fn falha_de_embedding_vira_contador() {
        struct SempreFalha;

        #[async_trait]
        impl crate::embeddings::EmbeddingProvider for SempreFalha {
            fn provider_id(&self) -> &str {
                "provider-de-teste"
            }
            fn model(&self) -> &str {
                "modelo"
            }
            async fn embed_documents(
                &self,
                _t: &[String],
            ) -> garraia_common::Result<Vec<Vec<f32>>> {
                Err(garraia_common::Error::Agent("fora do ar".into()))
            }
            async fn embed_query(&self, _t: &str) -> garraia_common::Result<Vec<f32>> {
                Err(garraia_common::Error::Agent("fora do ar".into()))
            }
            async fn health_check(&self) -> garraia_common::Result<bool> {
                Ok(false)
            }
        }

        let store = Arc::new(garraia_db::MemoryStore::in_memory_with_vectors().expect("store"));
        let mut rt = AgentRuntime::new();
        rt.set_memory_provider(store);
        rt.set_embedding_provider(Arc::new(SempreFalha));

        let emitidas = metricas_emitidas(|| async {
            rt.remember_turn("s1", None, None, "meu nome e Michel e moro na Florida", "")
                .await
                .expect("remember_turn");
        })
        .await;

        assert!(
            emitidas.iter().any(|m| m
                == "garraia_memory_embed_failures_total{operation=document,provider=provider-de-teste}"),
            "falha nao virou contador: {emitidas:?}"
        );
        assert!(
            emitidas
                .iter()
                .any(|m| m == "garraia_memory_ingested_total{outcome=failed}"),
            "desfecho `failed` nao foi contado: {emitidas:?}"
        );
    }

    /// Os quatro desfechos precisam ser distinguiveis. `no_provider` e
    /// `failed` sao a diferenca entre "ninguem configurou" e "configurou e
    /// esta quebrado" — a pergunta que o operador faz primeiro.
    #[tokio::test]
    async fn sem_provider_e_desfecho_proprio_nao_falha() {
        let store = Arc::new(garraia_db::MemoryStore::in_memory_with_vectors().expect("store"));
        let mut rt = AgentRuntime::new();
        rt.set_memory_provider(store);
        // Sem `set_embedding_provider`.

        let emitidas = metricas_emitidas(|| async {
            rt.remember_turn("s1", None, None, "um fato de verdade para lembrar", "")
                .await
                .expect("remember_turn");
        })
        .await;

        assert!(
            emitidas
                .iter()
                .any(|m| m == "garraia_memory_ingested_total{outcome=no_provider}"),
            "{emitidas:?}"
        );
        assert!(
            !emitidas.iter().any(|m| m.contains("embed_failures")),
            "sem provider nao e falha do provider: {emitidas:?}"
        );
    }

    /// Ruido tem desfecho proprio (#952). Sem isso, o operador veria o total
    /// de entradas sem vetor subir e nao teria como saber se e defeito ou
    /// politica.
    #[tokio::test]
    async fn ruido_tem_desfecho_proprio_e_nao_chama_o_provider() {
        let store = Arc::new(garraia_db::MemoryStore::in_memory_with_vectors().expect("store"));
        let embeddings = Arc::new(ContandoEmbeddings(std::sync::atomic::AtomicUsize::new(0)));
        let mut rt = AgentRuntime::new();
        rt.set_memory_provider(store);
        rt.set_embedding_provider(embeddings.clone());

        let emitidas = metricas_emitidas(|| async {
            rt.remember_turn("s1", None, None, "oi", "bom dia")
                .await
                .expect("remember_turn");
        })
        .await;

        assert!(
            emitidas
                .iter()
                .any(|m| m == "garraia_memory_ingested_total{outcome=noise}"),
            "{emitidas:?}"
        );
        assert_eq!(chamadas(&embeddings), 0, "ruido nao pode ir ao provider");
        assert!(
            !emitidas.iter().any(|m| m.contains("embed_latency")),
            "nao houve chamada, nao pode haver latencia: {emitidas:?}"
        );
    }

    /// O caminho feliz: latencia medida por provider e por operacao, e o
    /// desfecho contado como `embedded`.
    #[tokio::test]
    async fn sucesso_mede_latencia_por_provider_e_operacao() {
        let store = Arc::new(garraia_db::MemoryStore::in_memory_with_vectors().expect("store"));
        let mut rt = AgentRuntime::new();
        rt.set_memory_provider(store);
        rt.set_embedding_provider(Arc::new(ContandoEmbeddings(
            std::sync::atomic::AtomicUsize::new(0),
        )));

        let emitidas = metricas_emitidas(|| async {
            rt.remember_turn("s1", None, None, "um fato de verdade para lembrar", "")
                .await
                .expect("remember_turn");
            rt.recall_context("quem sou eu", Some("s1"), None, 5)
                .await
                .expect("recall");
        })
        .await;

        assert!(
            emitidas.iter().any(|m| m
                == "garraia_memory_embed_latency_seconds{operation=document,provider=contando}"),
            "{emitidas:?}"
        );
        assert!(
            emitidas
                .iter()
                .any(|m| m
                    == "garraia_memory_embed_latency_seconds{operation=query,provider=contando}"),
            "a consulta do recall nao foi medida: {emitidas:?}"
        );
        assert!(
            emitidas
                .iter()
                .any(|m| m == "garraia_memory_recall_latency_seconds"),
            "{emitidas:?}"
        );
        assert!(
            emitidas
                .iter()
                .any(|m| m == "garraia_memory_ingested_total{outcome=embedded}"),
            "{emitidas:?}"
        );
    }

    /// Nenhuma label pode carregar id de sessao, de usuario ou conteudo — e a
    /// explosao de cardinalidade que o docblock do `garraia-telemetry` descreve.
    #[tokio::test]
    async fn nenhuma_label_carrega_identificador_ou_conteudo() {
        let store = Arc::new(garraia_db::MemoryStore::in_memory_with_vectors().expect("store"));
        let mut rt = AgentRuntime::new();
        rt.set_memory_provider(store);
        rt.set_embedding_provider(Arc::new(ContandoEmbeddings(
            std::sync::atomic::AtomicUsize::new(0),
        )));

        let emitidas = metricas_emitidas(|| async {
            rt.remember_turn(
                "sessao-secreta-42",
                None,
                Some("usuario-secreto"),
                "minha senha do banco e 1234",
                "",
            )
            .await
            .expect("remember_turn");
        })
        .await;

        for m in &emitidas {
            for proibido in ["sessao-secreta", "usuario-secreto", "senha", "1234"] {
                assert!(!m.contains(proibido), "label vazou {proibido:?}: {m}");
            }
        }
        assert!(!emitidas.is_empty(), "o teste precisa ter emitido algo");
    }

    // ─── #952: ruido nao merece vetor ─────────────────────────────────────

    /// Provider de embeddings que conta quantas vezes foi chamado.
    ///
    /// O contador e metade do teste: a politica nao so evita gravar o vetor,
    /// ela evita **pedir** o vetor. Numa conversa longa, uma ida ao provider
    /// por "ok" nao e pouco.
    struct ContandoEmbeddings(std::sync::atomic::AtomicUsize);

    #[async_trait]
    impl crate::embeddings::EmbeddingProvider for ContandoEmbeddings {
        fn provider_id(&self) -> &str {
            "contando"
        }
        fn model(&self) -> &str {
            "modelo-de-teste"
        }
        async fn embed_documents(&self, texts: &[String]) -> garraia_common::Result<Vec<Vec<f32>>> {
            self.0
                .fetch_add(texts.len(), std::sync::atomic::Ordering::SeqCst);
            Ok(texts.iter().map(|_| vec![0.25, 0.5, 0.75]).collect())
        }
        async fn embed_query(&self, _text: &str) -> garraia_common::Result<Vec<f32>> {
            Ok(vec![0.25, 0.5, 0.75])
        }
        async fn health_check(&self) -> garraia_common::Result<bool> {
            Ok(true)
        }
    }

    async fn runtime_com_memoria(
        policy: crate::memory_noise::NoisePolicy,
    ) -> (
        AgentRuntime,
        Arc<garraia_db::MemoryStore>,
        Arc<ContandoEmbeddings>,
    ) {
        let store = Arc::new(garraia_db::MemoryStore::in_memory_with_vectors().expect("store"));
        let embeddings = Arc::new(ContandoEmbeddings(std::sync::atomic::AtomicUsize::new(0)));
        let mut rt = AgentRuntime::new();
        rt.set_memory_provider(store.clone());
        rt.set_embedding_provider(embeddings.clone());
        rt.set_noise_policy(policy);
        (rt, store, embeddings)
    }

    fn chamadas(e: &ContandoEmbeddings) -> usize {
        e.0.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// O sintoma da issue: "oi" era gravado **com** vetor e disputava o
    /// top-K com memoria de verdade.
    #[tokio::test]
    async fn turno_de_ruido_e_gravado_sem_vetor_e_sem_ida_ao_provider() {
        let (rt, store, embeddings) =
            runtime_com_memoria(crate::memory_noise::NoisePolicy::default()).await;

        rt.remember_turn("s1", None, None, "oi", "bom dia")
            .await
            .expect("remember_turn");

        assert_eq!(chamadas(&embeddings), 0, "pediu vetor para ruido");
        let r = store.integrity_report().expect("report");
        assert_eq!(r.entries_with_embedding, 0);
        assert_eq!(r.entries_without_embedding, 2, "as duas seguem gravadas");
    }

    /// O outro lado da moeda, e o que impede o filtro de virar perda de
    /// memoria: conteudo de verdade continua ganhando vetor.
    #[tokio::test]
    async fn turno_de_verdade_continua_ganhando_vetor() {
        let (rt, store, embeddings) =
            runtime_com_memoria(crate::memory_noise::NoisePolicy::default()).await;

        rt.remember_turn(
            "s1",
            None,
            None,
            "meu nome e Michel e eu moro na Florida",
            "anotado: voce se chama Michel e mora na Florida",
        )
        .await
        .expect("remember_turn");

        assert_eq!(chamadas(&embeddings), 2);
        let r = store.integrity_report().expect("report");
        assert_eq!(r.entries_with_embedding, 2);
        assert_eq!(r.map_rows, 2, "as duas entraram no indice vetorial");
    }

    /// Um turno pode ter uma metade de ruido e outra de conteudo. Elas sao
    /// decididas separadamente — a pergunta "ok" nao pode derrubar a resposta
    /// que veio depois dela.
    #[tokio::test]
    async fn as_duas_metades_do_turno_sao_decididas_em_separado() {
        let (rt, store, embeddings) =
            runtime_com_memoria(crate::memory_noise::NoisePolicy::default()).await;

        rt.remember_turn(
            "s1",
            None,
            None,
            "ok",
            "o gateway sobe na porta 3888 e le a config de ~/.garraia",
        )
        .await
        .expect("remember_turn");

        assert_eq!(chamadas(&embeddings), 1);
        let r = store.integrity_report().expect("report");
        assert_eq!(r.entries_with_embedding, 1);
        assert_eq!(r.entries_without_embedding, 1);
    }

    /// Desligar a politica devolve o comportamento anterior ao #952, inteiro.
    #[tokio::test]
    async fn politica_desligada_embedda_ate_o_ruido() {
        let (rt, store, embeddings) =
            runtime_com_memoria(crate::memory_noise::NoisePolicy::disabled()).await;

        rt.remember_turn("s1", None, None, "oi", "bom dia")
            .await
            .expect("remember_turn");

        assert_eq!(chamadas(&embeddings), 2);
        assert_eq!(
            store
                .integrity_report()
                .expect("report")
                .entries_with_embedding,
            2
        );
    }

    /// A coluna `embedding_model` descreve o vetor. Uma entrada pulada por
    /// ruido nao tem vetor, entao nao pode sair dizendo por qual modelo foi
    /// indexada — foi essa mentira que o #948 corrigiu, e o filtro novo nao
    /// pode reintroduzi-la.
    #[tokio::test]
    async fn entrada_pulada_nao_finge_ter_modelo() {
        let (rt, store, _) = runtime_com_memoria(crate::memory_noise::NoisePolicy::default()).await;

        rt.remember_turn("s1", None, None, "ok", "valeu")
            .await
            .expect("remember_turn");

        for entrada in store.recent_entries(10).expect("recent") {
            assert!(entrada.embedding.is_none());
            assert!(
                entrada.embedding_model.is_none(),
                "linha sem vetor anunciando modelo: {:?}",
                entrada.embedding_model
            );
        }
    }

    // ─── issue #924: o inventario de tools nao pode congelar no boot ───────

    use crate::tools::{ToolContext, ToolOutput};
    use async_trait::async_trait;

    struct StubTool(&'static str);

    #[async_trait]
    impl Tool for StubTool {
        fn name(&self) -> &str {
            self.0
        }
        fn description(&self) -> &str {
            "stub"
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        async fn execute(
            &self,
            _c: &ToolContext,
            _i: serde_json::Value,
        ) -> garraia_common::Result<ToolOutput> {
            Ok(ToolOutput::success("ok"))
        }
    }

    fn stub(name: &'static str) -> Box<dyn Tool> {
        Box::new(StubTool(name))
    }

    /// O cenario exato do relato: o boot registra so as nativas porque o
    /// connect do servidor MCP perdeu a corrida, o servidor conecta depois, e
    /// o runtime tem de acabar com as duas metades — nao com seis tools e um
    /// servidor reportando catorze.
    #[test]
    fn late_connecting_server_still_lands_in_the_runtime() {
        let rt = AgentRuntime::new();
        rt.register_tool(stub("bash"));
        rt.register_tool(stub("file_read"));
        assert_eq!(rt.tool_names().len(), 2);

        // O health monitor reconecta e sincroniza.
        let delta = rt.replace_mcp_tools(
            "filesystem",
            vec![stub("filesystem__read_file"), stub("filesystem__list_dir")],
        );
        assert_eq!(delta.removed, 0);
        assert_eq!(delta.added, 2);

        assert_eq!(rt.tool_names().len(), 4);
        // E, o que importa de verdade: o LLM as ve.
        let defs: Vec<String> = rt.tool_definitions().into_iter().map(|d| d.name).collect();
        assert!(defs.contains(&"filesystem__read_file".to_string()));
        assert!(rt.find_tool("filesystem__list_dir").is_some());
    }

    /// Idempotencia: rodar a cada 30s nao pode acumular duplicatas. Como
    /// `find_tool` e uma varredura linear, duplicatas se sombreariam em
    /// silencio em vez de dar erro.
    #[test]
    fn repeated_sync_does_not_duplicate() {
        let rt = AgentRuntime::new();
        rt.register_tool(stub("bash"));

        for _ in 0..5 {
            rt.replace_mcp_tools("fs", vec![stub("fs__a"), stub("fs__b")]);
        }

        assert_eq!(rt.tool_names().len(), 3);
        assert_eq!(rt.tool_names().iter().filter(|n| *n == "fs__a").count(), 1);
    }

    /// Um servidor que volta com inventario menor tem de encolher, e nunca
    /// levar junto as tools nativas nem as de outro servidor.
    #[test]
    fn sync_is_scoped_to_one_server_and_can_shrink() {
        let rt = AgentRuntime::new();
        rt.register_tool(stub("bash"));
        rt.replace_mcp_tools("fs", vec![stub("fs__a"), stub("fs__b"), stub("fs__c")]);
        rt.replace_mcp_tools("git", vec![stub("git__log")]);
        assert_eq!(rt.tool_names().len(), 5);

        let delta = rt.replace_mcp_tools("fs", vec![stub("fs__a")]);
        assert_eq!(delta.removed, 3);
        assert_eq!(delta.added, 1);

        let names = rt.tool_names();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"bash".to_string()), "nativa preservada");
        assert!(
            names.contains(&"git__log".to_string()),
            "outro servidor intacto"
        );
        assert!(
            !names.contains(&"fs__b".to_string()),
            "tool sumida foi removida"
        );
    }

    /// Um servidor que desaparece por completo esvazia so a propria fatia.
    #[test]
    fn empty_inventory_clears_only_that_server() {
        let rt = AgentRuntime::new();
        rt.register_tool(stub("bash"));
        rt.replace_mcp_tools("fs", vec![stub("fs__a")]);

        let delta = rt.replace_mcp_tools("fs", Vec::new());
        assert_eq!(delta.removed, 1);
        assert_eq!(delta.added, 0);
        assert_eq!(rt.tool_names(), vec!["bash".to_string()]);
    }

    /// O inventario distingue origem — e o que torna as duas contagens da API
    /// conferiveis em vez de misteriosas.
    #[test]
    fn inventory_reports_source_and_server() {
        let rt = AgentRuntime::new();
        rt.register_tool(stub("bash"));
        rt.replace_mcp_tools("filesystem", vec![stub("filesystem__read_file")]);

        let inv = rt.tool_inventory();
        let native = inv.iter().find(|t| t.name == "bash").unwrap();
        assert_eq!(native.source, "native");
        assert!(native.server.is_none());

        let mcp = inv
            .iter()
            .find(|t| t.name == "filesystem__read_file")
            .unwrap();
        assert_eq!(mcp.source, "mcp");
        assert_eq!(mcp.server.as_deref(), Some("filesystem"));
    }

    /// `register_tool` toma `&self`: o runtime ja esta dentro de um `Arc`
    /// quando as tools de schedule sao registradas, e antes disso o
    /// `Arc::get_mut` pulava o registro em silencio se o rc fosse > 1.
    #[test]
    fn registration_works_through_a_shared_arc() {
        let rt = Arc::new(AgentRuntime::new());
        let clone = Arc::clone(&rt);
        assert_eq!(Arc::strong_count(&rt), 2);

        clone.register_tool(stub("schedule_heartbeat"));
        assert!(rt.find_tool("schedule_heartbeat").is_some());
    }

    /// Test that AgentRuntime can be created with an empty/default config without crashing.
    /// This test verifies the "empty config" scenario is handled safely.
    #[test]
    fn build_agent_runtime_empty_config_no_crash() {
        // Create a runtime with default/empty configuration
        let runtime = AgentRuntime::new();

        // Verify basic state is correct for empty config
        assert!(runtime.providers.read().unwrap().is_empty());
        assert!(runtime.default_provider.read().unwrap().is_none());
        assert!(runtime.memory.is_none());
        assert!(runtime.embeddings.is_none());
        assert!(runtime.tool_names().is_empty());
        assert!(runtime.system_prompt.is_none());
        assert!(runtime.max_tokens.is_none());
        assert!(runtime.max_context_tokens.is_none());

        // Verify methods that could crash with empty config don't panic
        let _ = runtime.provider_ids();
        let _ = runtime.default_provider_id();
        let _ = runtime.has_memory_provider();
        let _ = runtime.has_embedding_provider();
        let _ = runtime.list_tool_info();
        let _ = runtime.system_prompt();

        // Verify getting a non-existent provider returns None, not a crash
        let _ = runtime.get_provider("nonexistent");
        let _ = runtime.default_provider();

        // Test setting values on empty runtime doesn't panic
        let mut runtime = runtime;
        runtime.set_system_prompt("test prompt".to_string());
        runtime.set_max_tokens(1000);
        runtime.set_max_context_tokens(8000);

        assert_eq!(runtime.system_prompt(), Some("test prompt"));
        assert_eq!(runtime.max_tokens, Some(1000));
        assert_eq!(runtime.max_context_tokens, Some(8000));
    }

    /// Test that AgentRuntime Default trait works correctly.
    #[test]
    fn agent_runtime_default_is_empty() {
        let runtime = AgentRuntime::default();

        // Same checks as above but using Default
        assert!(runtime.providers.read().unwrap().is_empty());
        assert!(runtime.default_provider.read().unwrap().is_none());
    }
}

/// Rough token estimate: ~4 characters per token.
fn estimate_tokens(
    messages: &[ChatMessage],
    system: &Option<String>,
    tools: &[ToolDefinition],
) -> usize {
    let mut chars: usize = 0;
    if let Some(s) = system {
        chars += s.len();
    }
    for msg in messages {
        match &msg.content {
            MessagePart::Text(t) => chars += t.len(),
            MessagePart::Parts(parts) => {
                for part in parts {
                    match part {
                        ContentBlock::Text { text } => chars += text.len(),
                        ContentBlock::ToolUse { input, .. } => chars += input.to_string().len(),
                        ContentBlock::ToolResult { content, .. } => chars += content.len(),
                        ContentBlock::Image { .. } => chars += 1000,
                    }
                }
            }
        }
    }
    for tool in tools {
        chars += tool.description.len() + tool.input_schema.to_string().len();
    }
    chars / 4
}

/// Drop the oldest messages until the estimated token count fits the budget.
/// Always keeps at least the last message (the current user input).
/// Removes messages in pairs (assistant + tool-result) to avoid breaking
/// the conversation protocol required by LLM APIs.
fn trim_messages_to_budget(
    messages: &mut Vec<ChatMessage>,
    system: &Option<String>,
    tools: &[ToolDefinition],
    max_tokens: usize,
) {
    while messages.len() > 1 && estimate_tokens(messages, system, tools) > max_tokens {
        let has_tool_use = matches!(
            &messages[0].content,
            MessagePart::Parts(parts) if parts.iter().any(|b| matches!(b, ContentBlock::ToolUse { .. }))
        );

        messages.remove(0);

        if has_tool_use && messages.len() > 1 {
            let is_tool_result = matches!(
                &messages[0].content,
                MessagePart::Parts(parts) if parts.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. }))
            );
            if is_tool_result {
                messages.remove(0);
            }
        }
    }
}

fn extract_text(content: &[ContentBlock]) -> String {
    let text = content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    if text.trim().is_empty() {
        return "[no textual response provided by the model]".to_string();
    }

    text
}
