use std::sync::{Arc, RwLock};

use futures::StreamExt;
use futures::future::join_all;
use garraia_common::{Error, Result};
use garraia_db::{MemoryEntry, MemoryProvider, MemoryRole, NewMemoryEntry, RecallQuery};
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::{info, instrument, warn};

use crate::embeddings::EmbeddingProvider;
use crate::execution_budget::ExecutionBudget;
use crate::memory_extractor::LlmMemoryExtractor;
use crate::providers::{
    ChatMessage, ChatRole, ContentBlock, LlmProvider, LlmRequest, MessagePart, StreamEvent,
    ToolDefinition,
};
use crate::tools::{Tool, ToolContext, ToolOutput};

/// Manages agent sessions, tool execution, and LLM provider routing.
pub struct AgentRuntime {
    providers: RwLock<Vec<Arc<dyn LlmProvider>>>,
    default_provider: RwLock<Option<String>>,
    memory: Option<Arc<dyn MemoryProvider>>,
    embeddings: Option<Arc<dyn EmbeddingProvider>>,
    tools: Vec<Box<dyn Tool>>,
    system_prompt: Option<String>,
    max_tokens: Option<u32>,
    max_context_tokens: Option<usize>,
    memory_extractor: LlmMemoryExtractor,
}

impl AgentRuntime {
    pub fn new() -> Self {
        Self {
            providers: RwLock::new(Vec::new()),
            default_provider: RwLock::new(None),
            memory: None,
            embeddings: None,
            tools: Vec::new(),
            system_prompt: None,
            max_tokens: None,
            max_context_tokens: None,
            memory_extractor: LlmMemoryExtractor::new(),
        }
    }

    pub fn system_prompt(&self) -> Option<&str> {
        self.system_prompt.as_deref()
    }

    pub fn set_system_prompt(&mut self, prompt: String) {
        self.system_prompt = Some(prompt);
    }

    pub fn set_max_tokens(&mut self, max_tokens: u32) {
        self.max_tokens = Some(max_tokens);
    }

    pub fn set_max_context_tokens(&mut self, max_context_tokens: usize) {
        self.max_context_tokens = Some(max_context_tokens);
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

        let user_embedding = self.embed_document(user_input).await;
        let assistant_embedding = self.embed_document(assistant_output).await;

        memory
            .remember(NewMemoryEntry {
                session_id: session_id.to_string(),
                channel_id: None,
                user_id: user_id.map(|s| s.to_string()),
                continuity_key: continuity_key.map(|s| s.to_string()),
                role: MemoryRole::User,
                content: user_input.to_string(),
                embedding: user_embedding,
                embedding_model: self.embedding_model(),
                metadata: serde_json::json!({ "kind": "turn_user" }),
            })
            .await?;

        memory
            .remember(NewMemoryEntry {
                session_id: session_id.to_string(),
                channel_id: None,
                user_id: user_id.map(|s| s.to_string()),
                continuity_key: continuity_key.map(|s| s.to_string()),
                role: MemoryRole::Assistant,
                content: assistant_output.to_string(),
                embedding: assistant_embedding,
                embedding_model: self.embedding_model(),
                metadata: serde_json::json!({ "kind": "turn_assistant" }),
            })
            .await?;

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

        let query_embedding = self.embed_query(query_text).await;

        memory
            .recall(RecallQuery {
                query_text: Some(query_text.to_string()),
                query_embedding,
                session_id: session_id.map(|s| s.to_string()),
                continuity_key: continuity_key.map(|s| s.to_string()),
                limit,
            })
            .await
    }

    pub fn register_tool(&mut self, tool: Box<dyn Tool>) {
        info!("registered tool: {}", tool.name());
        self.tools.push(tool);
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .map(|t| ToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                input_schema: t.input_schema(),
            })
            .collect()
    }

    fn find_tool(&self, name: &str) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .find(|t| t.name() == name)
            .map(|t| t.as_ref())
    }

    /// Run the full conversation loop: recall context, call LLM, execute tools, return response.
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
    pub async fn process_message_with_context(
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
            false,
        )
        .await
    }

    /// Process a scheduled heartbeat message. Tools receive `is_heartbeat = true`
    /// so that recursive scheduling is blocked.
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
        let provider: Arc<dyn LlmProvider> = if let Some(pid) = provider_id {
            self.get_provider(pid)
                .ok_or_else(|| Error::Agent(format!("provider '{pid}' not found")))?
        } else {
            self.default_provider()
                .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?
        };

        let effective_system_prompt = system_prompt_override
            .map(|s| s.to_string())
            .or_else(|| self.system_prompt.clone());
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

        let mut messages: Vec<ChatMessage> = conversation_history.to_vec();
        messages.push(ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text(user_text.to_string()),
        });

        let max_ctx = self.max_context_tokens.unwrap_or(100_000);
        trim_messages_to_budget(&mut messages, &system, &tool_defs, max_ctx);

        let mut budget = ExecutionBudget::padrao();
        
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

            let response = provider.complete(&request).await?;

            let has_tool_use = response
                .content
                .iter()
                .any(|block| matches!(block, ContentBlock::ToolUse { .. }));

            if !has_tool_use {
                let final_text = extract_text(&response.content);
                if let Err(e) = self
                    .remember_turn(session_id, continuity_key, user_id, user_text, &final_text)
                    .await
                {
                    warn!("failed to store turn in memory: {}", e);
                }
                return Ok(final_text);
            }

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
                    };
                    
                    // registra chamada com payload para detecção de loop por assinatura
                    budget.registrar_chamada(name, input);
                    
                    // detecta loop
                    if budget.detectar_loop_ferramenta() {
                        return Err(Error::Agent(format!(
                            "tool loop detected: {}",
                            name
                        )));
                    }
                    
                    // executa com timeout
                    let output = match self.find_tool(name) {
                        Some(tool) => {
                            match timeout(
                                budget.timeout(),
                                tool.execute(&context, input.clone())
                            ).await {
                                Ok(result) => {
                                    result.unwrap_or_else(|e| ToolOutput::error(e.to_string()))
                                }
                                Err(_) => {
                                    ToolOutput::error(format!(
                                        "tool timeout: {}",
                                        name
                                    ))
                                }
                            }
                        }
                        None => ToolOutput::error(format!("unknown tool: {}", name)),
                    };
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

    #[instrument(skip(self, conversation_history), fields(provider_id, continuity_key = ?continuity_key))]
    async fn process_message_impl(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
        continuity_key: Option<&str>,
        user_id: Option<&str>,
        is_heartbeat: bool,
    ) -> Result<String> {
        // Debug: Log system prompt presence
        if let Some(prompt) = &self.system_prompt {
            info!("system_prompt present, length: {}", prompt.len());
            if prompt.contains("Fatos do Usuário") {
                info!("system_prompt contains user facts!");
            }
        } else {
            warn!("system_prompt is None!");
        }

        let provider: Arc<dyn LlmProvider> = self
            .default_provider()
            .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?;

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

        let system = match (&self.system_prompt, memory_context) {
            (Some(prompt), Some(ctx)) => Some(format!("{prompt}\n\n{ctx}")),
            (Some(prompt), None) => Some(prompt.clone()),
            (None, Some(ctx)) => Some(ctx),
            (None, None) => None,
        };

        let tool_defs = self.tool_definitions();

        let mut messages: Vec<ChatMessage> = conversation_history.to_vec();
        messages.push(ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text(user_text.to_string()),
        });

        // Trim conversation history to fit context window
        let max_ctx = self.max_context_tokens.unwrap_or(100_000);
        trim_messages_to_budget(&mut messages, &system, &tool_defs, max_ctx);

        let mut budget = ExecutionBudget::padrao();
        
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
                model: String::new(),
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
                                let _ = memory.remember(NewMemoryEntry {
                                    session_id: session_id.to_string(),
                                    channel_id: None,
                                    user_id: user_id.map(|s| s.to_string()),
                                    continuity_key: continuity_key.map(|s| s.to_string()),
                                    role: MemoryRole::User,
                                    content,
                                    embedding,
                                    embedding_model: self.embedding_model(),
                                    metadata: serde_json::json!({ "kind": "learned_fact" }),
                                }).await;
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
            for block in &response.content {
                if let ContentBlock::ToolUse { id, name, input } = block {
                    let context = ToolContext {
                        session_id: session_id.to_string(),
                        user_id: user_id.map(|s| s.to_string()),
                        is_heartbeat,
                    };
                    
                    // registra chamada com payload para detecção de loop por assinatura
                    budget.registrar_chamada(name, input);
                    
                    // detecta loop
                    if budget.detectar_loop_ferramenta() {
                        return Err(Error::Agent(format!(
                            "tool loop detected: {}",
                            name
                        )));
                    }
                    
                    // executa com timeout
                    let output = match self.find_tool(name) {
                        Some(tool) => {
                            match timeout(
                                budget.timeout(),
                                tool.execute(&context, input.clone())
                            ).await {
                                Ok(result) => {
                                    result.unwrap_or_else(|e| ToolOutput::error(e.to_string()))
                                }
                                Err(_) => {
                                    ToolOutput::error(format!(
                                        "tool timeout: {}",
                                        name
                                    ))
                                }
                            }
                        }
                        None => ToolOutput::error(format!("unknown tool: {}", name)),
                    };
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
    ) -> Result<String> {
        self.process_message_streaming_with_context(
            session_id,
            user_text,
            conversation_history,
            delta_tx,
            None,
            None,
        )
        .await
    }

    /// Streaming variant with continuity/user context for shared memory.
    #[instrument(skip(self, conversation_history, delta_tx), fields(provider_id, continuity_key = ?continuity_key))]
    pub async fn process_message_streaming_with_context(
        &self,
        session_id: &str,
        user_text: &str,
        conversation_history: &[ChatMessage],
        delta_tx: mpsc::Sender<String>,
        continuity_key: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<String> {
        self.process_message_streaming_with_agent_config(
            session_id,
            user_text,
            conversation_history,
            delta_tx,
            continuity_key,
            user_id,
            None,
            None,
            None,
            None,
        ).await
    }

    /// Streaming variant with explicit agent config overrides (for multi-agent routing or dynamic models).
    #[allow(clippy::too_many_arguments)]
    #[instrument(skip(self, conversation_history, delta_tx), fields(provider_id, continuity_key = ?continuity_key))]
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
        let provider: Arc<dyn LlmProvider> = if let Some(pid) = provider_id {
            self.get_provider(pid)
                .ok_or_else(|| Error::Agent(format!("provider '{pid}' not found")))?
        } else {
            self.default_provider()
                .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?
        };

        let effective_system_prompt = system_prompt_override
            .map(|s| s.to_string())
            .or_else(|| self.system_prompt.clone());
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

        let mut messages: Vec<ChatMessage> = conversation_history.to_vec();
        messages.push(ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text(user_text.to_string()),
        });

        let max_ctx = self.max_context_tokens.unwrap_or(100_000);
        trim_messages_to_budget(&mut messages, &system, &tool_defs, max_ctx);

        let mut full_response = String::new();

        let mut budget = ExecutionBudget::padrao();
        
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

            // Try streaming; fall back to non-streaming if not supported
            let stream_result = provider.stream_complete(&request).await;

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
                                let _ = delta_tx.send(text).await;
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
                    for (id, name, input_json) in &tool_uses {
                        let input: serde_json::Value =
                            serde_json::from_str(input_json).unwrap_or_default();
                        let context = ToolContext {
                            session_id: session_id.to_string(),
                            user_id: user_id.map(|s| s.to_string()),
                            is_heartbeat: false,
                        };
                        
                        // registra chamada com payload para detecção de loop por assinatura
                        budget.registrar_chamada(name, &input);
                        
                        // detecta loop
                        if budget.detectar_loop_ferramenta() {
                            return Err(Error::Agent(format!(
                                "tool loop detected: {}",
                                name
                            )));
                        }
                        
                        // executa com timeout
                        let output = match self.find_tool(name) {
                            Some(tool) => {
                                match timeout(
                                    budget.timeout(),
                                    tool.execute(&context, input)
                                ).await {
                                    Ok(result) => {
                                        result.unwrap_or_else(|e| ToolOutput::error(e.to_string()))
                                    }
                                    Err(_) => {
                                        ToolOutput::error(format!(
                                            "tool timeout: {}",
                                            name
                                        ))
                                    }
                                }
                            }
                            None => ToolOutput::error(format!("unknown tool: {}", name)),
                        };
                        tool_results.push(ContentBlock::ToolResult {
                            tool_use_id: id.clone(),
                            content: output.content,
                        });
                    }

                    messages.push(ChatMessage {
                        role: ChatRole::User,
                        content: MessagePart::Parts(tool_results),
                    });

                    // Add separator between iterations
                    if !full_response.is_empty() {
                        full_response.push_str("\n\n");
                        let _ = delta_tx.send("\n\n".to_string()).await;
                    }
                }
                Err(_) => {
                    // Streaming not supported — fall back to non-streaming
                    let response = provider.complete(&request).await?;

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
                        let _ = delta_tx.send(final_text.clone()).await;
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
                    for block in &response.content {
                        if let ContentBlock::ToolUse { id, name, input } = block {
                            let context = ToolContext {
                                session_id: session_id.to_string(),
                                user_id: user_id.map(|s| s.to_string()),
                                is_heartbeat: false,
                            };
                            
                            // registra chamada com payload para detecção de loop por assinatura
                            budget.registrar_chamada(name, input);
                            
                            // detecta loop
                            if budget.detectar_loop_ferramenta() {
                                return Err(Error::Agent(format!(
                                    "tool loop detected: {}",
                                    name
                                )));
                            }
                            
                            // executa com timeout
                            let output = match self.find_tool(name) {
                                Some(tool) => {
                                    match timeout(
                                        budget.timeout(),
                                        tool.execute(&context, input.clone())
                                    ).await {
                                        Ok(result) => {
                                            result.unwrap_or_else(|e| ToolOutput::error(e.to_string()))
                                        }
                                        Err(_) => {
                                            ToolOutput::error(format!(
                                                "tool timeout: {}",
                                                name
                                            ))
                                        }
                                    }
                                }
                                None => ToolOutput::error(format!("unknown tool: {}", name)),
                            };
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
        }
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

    async fn embed_document(&self, text: &str) -> Option<Vec<f32>> {
        let provider = self.embeddings.as_ref()?;
        provider
            .embed_documents(&[text.to_string()])
            .await
            .ok()
            .and_then(|mut v| v.pop())
    }

    async fn embed_query(&self, text: &str) -> Option<Vec<f32>> {
        let provider = self.embeddings.as_ref()?;
        provider.embed_query(text).await.ok()
    }

    fn embedding_model(&self) -> Option<String> {
        self.embeddings
            .as_ref()
            .map(|provider| provider.model().to_string())
    }

    /// Simple chat completion for use by memory extractor
    pub async fn chat_completion(
        &self,
        messages: Vec<ChatMessage>,
        _system: Option<String>,
        _model: Option<String>,
        _max_tokens: Option<u32>,
        _temperature: Option<f32>,
        _tools: Option<Vec<ToolDefinition>>,
    ) -> Result<String> {
        let provider = self
            .default_provider()
            .ok_or_else(|| Error::Agent("no LLM provider configured".into()))?;

        let request = LlmRequest {
            model: String::new(),
            messages,
            system: None,
            max_tokens: Some(4096),
            temperature: None,
            tools: Vec::new(),
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
fn trim_messages_to_budget(
    messages: &mut Vec<ChatMessage>,
    system: &Option<String>,
    tools: &[ToolDefinition],
    max_tokens: usize,
) {
    while messages.len() > 1 && estimate_tokens(messages, system, tools) > max_tokens {
        messages.remove(0);
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
